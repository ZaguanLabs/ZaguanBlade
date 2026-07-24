use crate::blade_protocol::{
    BladeEvent, BladeEventEnvelope, BladeEventTransport, ChatEvent, TerminalEvent,
};
use crate::protocol::ToolCall;
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Runtime};

const FRAME_FLUSH_INTERVAL: Duration = Duration::from_millis(12);
const CHAT_FLUSH_BYTES: usize = 8 * 1024;
const TERMINAL_FLUSH_BYTES: usize = 32 * 1024;
const METRIC_SAMPLE_CAP: usize = 512;

type EmitFn = Arc<dyn Fn(BladeEventTransport) + Send + Sync + 'static>;
type RefreshEmitFn = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatDeltaKind {
    Content,
    Reasoning,
}

#[derive(Clone, Copy)]
enum EmitClass {
    Immediate,
    ChatMessageDelta,
    ChatReasoningDelta,
    TerminalOutput,
    ToolUpdate,
    ToolActivity,
    ExplorerRefresh,
}

#[derive(Clone, Copy)]
enum FlushReason {
    Early,
    Interval,
    Disconnect,
    Manual,
}

#[derive(Default, Clone)]
struct LatencyMetricState {
    total_count: u64,
    total_delay_ms: u64,
    samples_ms: Vec<u64>,
}

impl LatencyMetricState {
    fn record(&mut self, delay_ms: u64) {
        self.total_count += 1;
        self.total_delay_ms += delay_ms;
        self.samples_ms.push(delay_ms);
        if self.samples_ms.len() > METRIC_SAMPLE_CAP {
            let drop_count = self.samples_ms.len() - METRIC_SAMPLE_CAP;
            self.samples_ms.drain(0..drop_count);
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        if self.total_count == 0 {
            return serde_json::json!({
                "count": 0,
                "avg": 0.0,
                "p50": 0,
                "p95": 0,
                "max": 0,
            });
        }

        let mut samples = self.samples_ms.clone();
        samples.sort_unstable();

        serde_json::json!({
            "count": self.total_count,
            "avg": self.total_delay_ms as f64 / self.total_count as f64,
            "p50": percentile(&samples, 0.50),
            "p95": percentile(&samples, 0.95),
            "max": samples.last().copied().unwrap_or(0),
        })
    }
}

#[derive(Default, Clone)]
struct EmitMetricState {
    queued_items: u64,
    queued_bytes: u64,
    coalesced_items: u64,
    emitted_envelopes: u64,
    emitted_payload_bytes: u64,
    latency_ms: LatencyMetricState,
}

impl EmitMetricState {
    fn snapshot(&self) -> serde_json::Value {
        let coalescing_hit_rate = if self.queued_items == 0 {
            0.0
        } else {
            self.coalesced_items as f64 / self.queued_items as f64
        };

        serde_json::json!({
            "queued_items": self.queued_items,
            "queued_bytes": self.queued_bytes,
            "coalesced_items": self.coalesced_items,
            "coalescing_hit_rate": coalescing_hit_rate,
            "emitted_envelopes": self.emitted_envelopes,
            "emitted_payload_bytes": self.emitted_payload_bytes,
            "latency_ms": self.latency_ms.snapshot(),
        })
    }
}

#[derive(Default, Clone)]
struct SchedulerMetrics {
    flush_total: u64,
    flush_early: u64,
    flush_interval: u64,
    flush_disconnect: u64,
    flush_manual: u64,
    immediate: EmitMetricState,
    chat_message_delta: EmitMetricState,
    chat_reasoning_delta: EmitMetricState,
    terminal_output: EmitMetricState,
    tool_update: EmitMetricState,
    tool_activity: EmitMetricState,
    explorer_refresh: EmitMetricState,
}

impl SchedulerMetrics {
    fn bucket_mut(&mut self, class: EmitClass) -> &mut EmitMetricState {
        match class {
            EmitClass::Immediate => &mut self.immediate,
            EmitClass::ChatMessageDelta => &mut self.chat_message_delta,
            EmitClass::ChatReasoningDelta => &mut self.chat_reasoning_delta,
            EmitClass::TerminalOutput => &mut self.terminal_output,
            EmitClass::ToolUpdate => &mut self.tool_update,
            EmitClass::ToolActivity => &mut self.tool_activity,
            EmitClass::ExplorerRefresh => &mut self.explorer_refresh,
        }
    }

    fn record_queued(&mut self, class: EmitClass, bytes: usize) {
        let bucket = self.bucket_mut(class);
        bucket.queued_items += 1;
        bucket.queued_bytes += bytes as u64;
    }

    fn record_coalesced(&mut self, class: EmitClass) {
        self.bucket_mut(class).coalesced_items += 1;
    }

    fn record_emitted(&mut self, class: EmitClass, payload_bytes: usize, delay_ms: u64) {
        let bucket = self.bucket_mut(class);
        bucket.emitted_envelopes += 1;
        bucket.emitted_payload_bytes += payload_bytes as u64;
        bucket.latency_ms.record(delay_ms);
    }

    fn record_flush(&mut self, reason: FlushReason) {
        self.flush_total += 1;
        match reason {
            FlushReason::Early => self.flush_early += 1,
            FlushReason::Interval => self.flush_interval += 1,
            FlushReason::Disconnect => self.flush_disconnect += 1,
            FlushReason::Manual => self.flush_manual += 1,
        }
    }

    fn snapshot(&self, state: &SchedulerState) -> serde_json::Value {
        serde_json::json!({
            "flush": {
                "total": self.flush_total,
                "early": self.flush_early,
                "interval": self.flush_interval,
                "disconnect": self.flush_disconnect,
                "manual": self.flush_manual,
            },
            "pending": {
                "chat_items": state.pending_chat.len(),
                "terminal_streams": state.pending_terminal.len(),
                "tool_update_items": state.pending_tool_update.len(),
                "tool_activity_items": state.pending_tool_activity.len(),
                "explorer_refresh_pending": state.pending_explorer_refresh.is_some(),
            },
            "immediate": self.immediate.snapshot(),
            "chat_message_delta": self.chat_message_delta.snapshot(),
            "chat_reasoning_delta": self.chat_reasoning_delta.snapshot(),
            "terminal_output": self.terminal_output.snapshot(),
            "tool_update": self.tool_update.snapshot(),
            "tool_activity": self.tool_activity.snapshot(),
            "explorer_refresh": self.explorer_refresh.snapshot(),
        })
    }
}

#[derive(Clone)]
struct PendingChatDelta {
    emit: EmitFn,
    causality_id: Option<String>,
    message_id: String,
    kind: ChatDeltaKind,
    chunk: String,
    queued_at: Instant,
}

#[derive(Clone)]
struct PendingTerminalOutput {
    emit: EmitFn,
    terminal_id: String,
    data: String,
    queued_at: Instant,
}

#[derive(Clone)]
struct PendingToolActivity {
    emit: EmitFn,
    tool_name: String,
    file_path: String,
    action: String,
    tool_call_id: Option<String>,
    queued_at: Instant,
}

#[derive(Clone)]
struct PendingToolUpdate {
    emit: EmitFn,
    causality_id: Option<String>,
    message_id: String,
    tool_call_id: String,
    status: String,
    result: Option<String>,
    tool_call: Option<ToolCall>,
    queued_at: Instant,
}

#[derive(Clone)]
struct PendingExplorerRefresh {
    emit: RefreshEmitFn,
    queued_at: Instant,
}

struct PendingTransportEnvelope {
    class: EmitClass,
    envelope: BladeEventEnvelope,
    queued_at: Instant,
}

enum SchedulerCommand {
    EmitEnvelope {
        emit: EmitFn,
        envelope: BladeEventEnvelope,
        queued_at: Instant,
    },
    ResetChatStream,
    QueueChatDelta {
        emit: EmitFn,
        causality_id: Option<String>,
        message_id: String,
        kind: ChatDeltaKind,
        chunk: String,
        queued_at: Instant,
    },
    QueueTerminalOutput {
        emit: EmitFn,
        terminal_id: String,
        data: String,
        queued_at: Instant,
    },
    QueueToolUpdate {
        emit: EmitFn,
        causality_id: Option<String>,
        message_id: String,
        tool_call_id: String,
        status: String,
        result: Option<String>,
        tool_call: Option<ToolCall>,
        queued_at: Instant,
    },
    QueueToolActivity {
        emit: EmitFn,
        tool_name: String,
        file_path: String,
        action: String,
        tool_call_id: Option<String>,
        queued_at: Instant,
    },
    QueueExplorerRefresh {
        emit: RefreshEmitFn,
        queued_at: Instant,
    },
    MetricsSnapshot {
        reply: mpsc::Sender<serde_json::Value>,
    },
}

#[derive(Default)]
struct SchedulerState {
    chat_seq_by_message: HashMap<String, u64>,
    terminal_seq: HashMap<String, u64>,
    pending_chat: VecDeque<PendingChatDelta>,
    pending_terminal: HashMap<String, PendingTerminalOutput>,
    pending_tool_update: HashMap<String, PendingToolUpdate>,
    pending_tool_activity: HashMap<String, PendingToolActivity>,
    pending_explorer_refresh: Option<PendingExplorerRefresh>,
    metrics: SchedulerMetrics,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len().saturating_sub(1)) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len().saturating_sub(1))]
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn build_envelope(causality_id: Option<String>, event: BladeEvent) -> BladeEventEnvelope {
    BladeEventEnvelope {
        id: uuid::Uuid::new_v4(),
        timestamp: current_timestamp_ms(),
        causality_id,
        event,
    }
}

fn emitter_for<R: Runtime>(app: &tauri::AppHandle<R>) -> EmitFn {
    let app = app.clone();
    Arc::new(move |transport| {
        let _ = app.emit("blade-event", transport);
    })
}

fn refresh_emitter_for<R: Runtime>(app: &tauri::AppHandle<R>) -> RefreshEmitFn {
    let app = app.clone();
    Arc::new(move || {
        let _ = app.emit(crate::events::event_names::REFRESH_EXPLORER, ());
    })
}

fn emit_now(emit: &EmitFn, transport: BladeEventTransport) {
    emit(transport);
}

fn emit_refresh_now(emit: &RefreshEmitFn) {
    emit();
}

fn estimate_envelope_bytes(envelope: &BladeEventEnvelope) -> usize {
    serde_json::to_vec(envelope)
        .map(|payload| payload.len())
        .unwrap_or_default()
}

fn delay_ms(queued_at: Instant) -> u64 {
    queued_at.elapsed().as_millis() as u64
}

fn emit_tracked(
    state: &mut SchedulerState,
    emit: &EmitFn,
    class: EmitClass,
    envelope: BladeEventEnvelope,
    queued_at: Instant,
) {
    let payload_bytes = estimate_envelope_bytes(&envelope);
    state
        .metrics
        .record_emitted(class, payload_bytes, delay_ms(queued_at));
    emit_now(emit, BladeEventTransport::Single(envelope));
}

fn emit_batch_tracked(
    state: &mut SchedulerState,
    emit: &EmitFn,
    batch: Vec<PendingTransportEnvelope>,
) {
    if batch.is_empty() {
        return;
    }

    for pending in &batch {
        state.metrics.record_emitted(
            pending.class,
            estimate_envelope_bytes(&pending.envelope),
            delay_ms(pending.queued_at),
        );
    }

    let transport = if batch.len() == 1 {
        BladeEventTransport::Single(
            batch
                .into_iter()
                .next()
                .expect("single batch item")
                .envelope,
        )
    } else {
        BladeEventTransport::Batch(batch.into_iter().map(|pending| pending.envelope).collect())
    };

    emit_now(emit, transport);
}

fn emit_refresh_tracked(
    state: &mut SchedulerState,
    emit: &RefreshEmitFn,
    class: EmitClass,
    queued_at: Instant,
) {
    state.metrics.record_emitted(class, 0, delay_ms(queued_at));
    emit_refresh_now(emit);
}

fn sender() -> &'static mpsc::Sender<SchedulerCommand> {
    static SENDER: OnceLock<mpsc::Sender<SchedulerCommand>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<SchedulerCommand>();
        std::thread::Builder::new()
            .name("blade-event-scheduler".to_string())
            .spawn(move || scheduler_loop(rx))
            .expect("failed to spawn blade-event-scheduler thread");
        tx
    })
}

fn scheduler_loop(rx: mpsc::Receiver<SchedulerCommand>) {
    let mut state = SchedulerState::default();

    loop {
        // When idle (no pending work), block indefinitely on recv() to avoid
        // ~83 unnecessary wakeups/sec from recv_timeout(12ms). Only use
        // recv_timeout when there's pending data that needs timely flushing.
        if !has_pending(&state) {
            match rx.recv() {
                Ok(command) => {
                    handle_command(&mut state, command);
                    if should_flush_early(&state) {
                        flush(&mut state, FlushReason::Early);
                    } else if should_flush_on_age(&state) {
                        flush(&mut state, FlushReason::Interval);
                    }
                }
                Err(_) => {
                    flush(&mut state, FlushReason::Disconnect);
                    break;
                }
            }
        } else {
            match rx.recv_timeout(FRAME_FLUSH_INTERVAL) {
                Ok(command) => {
                    handle_command(&mut state, command);
                    if should_flush_early(&state) {
                        flush(&mut state, FlushReason::Early);
                    } else if should_flush_on_age(&state) {
                        flush(&mut state, FlushReason::Interval);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => flush(&mut state, FlushReason::Interval),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    flush(&mut state, FlushReason::Disconnect);
                    break;
                }
            }
        }
    }
}

fn should_flush_early(state: &SchedulerState) -> bool {
    if state
        .pending_chat
        .front()
        .map(|pending| !state.chat_seq_by_message.contains_key(&pending.message_id))
        .unwrap_or(false)
    {
        return true;
    }

    let chat_bytes = state
        .pending_chat
        .iter()
        .map(|pending| pending.chunk.len())
        .sum::<usize>();
    if chat_bytes >= CHAT_FLUSH_BYTES {
        return true;
    }

    let terminal_bytes = state
        .pending_terminal
        .values()
        .map(|pending| pending.data.len())
        .sum::<usize>();
    terminal_bytes >= TERMINAL_FLUSH_BYTES
}

fn should_flush_on_age(state: &SchedulerState) -> bool {
    oldest_pending_age(state)
        .map(|age| age >= FRAME_FLUSH_INTERVAL)
        .unwrap_or(false)
}

fn oldest_pending_age(state: &SchedulerState) -> Option<Duration> {
    let now = Instant::now();
    let oldest_chat = state
        .pending_chat
        .front()
        .map(|pending| now.saturating_duration_since(pending.queued_at));
    let oldest_terminal = state
        .pending_terminal
        .values()
        .map(|pending| now.saturating_duration_since(pending.queued_at))
        .min();
    let oldest_tool_update = state
        .pending_tool_update
        .values()
        .map(|pending| now.saturating_duration_since(pending.queued_at))
        .min();
    let oldest_tool_activity = state
        .pending_tool_activity
        .values()
        .map(|pending| now.saturating_duration_since(pending.queued_at))
        .min();
    let oldest_explorer_refresh = state
        .pending_explorer_refresh
        .as_ref()
        .map(|pending| now.saturating_duration_since(pending.queued_at));

    [
        oldest_chat,
        oldest_terminal,
        oldest_tool_update,
        oldest_tool_activity,
        oldest_explorer_refresh,
    ]
    .into_iter()
    .flatten()
    .max()
}

fn has_pending(state: &SchedulerState) -> bool {
    !state.pending_chat.is_empty()
        || !state.pending_terminal.is_empty()
        || !state.pending_tool_update.is_empty()
        || !state.pending_tool_activity.is_empty()
        || state.pending_explorer_refresh.is_some()
}

fn estimate_tool_update_bytes(
    message_id: &str,
    tool_call_id: &str,
    status: &str,
    result: &Option<String>,
    tool_call: &Option<ToolCall>,
) -> usize {
    message_id.len()
        + tool_call_id.len()
        + status.len()
        + result.as_ref().map(|value| value.len()).unwrap_or_default()
        + tool_call
            .as_ref()
            .and_then(|value| serde_json::to_vec(value).ok())
            .map(|bytes| bytes.len())
            .unwrap_or_default()
}

fn handle_command(state: &mut SchedulerState, command: SchedulerCommand) {
    match command {
        SchedulerCommand::EmitEnvelope {
            emit,
            envelope,
            queued_at,
        } => {
            // "Immediate" is a latency class, not permission to overtake
            // stream data already accepted by this FIFO worker. In
            // particular, MessageCompleted must never reach the frontend
            // before the final queued text/reasoning delta.
            flush(state, FlushReason::Manual);
            emit_tracked(state, &emit, EmitClass::Immediate, envelope, queued_at);
        }
        SchedulerCommand::ResetChatStream => {
            flush(state, FlushReason::Manual);
            state.chat_seq_by_message.clear();
            state.pending_chat.clear();
        }
        SchedulerCommand::QueueChatDelta {
            emit,
            causality_id,
            message_id,
            kind,
            chunk,
            queued_at,
        } => {
            if chunk.is_empty() {
                return;
            }

            let class = match kind {
                ChatDeltaKind::Content => EmitClass::ChatMessageDelta,
                ChatDeltaKind::Reasoning => EmitClass::ChatReasoningDelta,
            };
            state.metrics.record_queued(class, chunk.len());

            if let Some(last) = state.pending_chat.back_mut() {
                if last.message_id == message_id
                    && last.kind == kind
                    && last.causality_id == causality_id
                {
                    last.chunk.push_str(&chunk);
                    state.metrics.record_coalesced(class);
                    return;
                }
            }

            state.pending_chat.push_back(PendingChatDelta {
                emit,
                causality_id,
                message_id,
                kind,
                chunk,
                queued_at,
            });
        }
        SchedulerCommand::QueueTerminalOutput {
            emit,
            terminal_id,
            data,
            queued_at,
        } => {
            if data.is_empty() {
                return;
            }

            state
                .metrics
                .record_queued(EmitClass::TerminalOutput, data.len());

            let mut was_coalesced = false;
            state
                .pending_terminal
                .entry(terminal_id.clone())
                .and_modify(|pending| {
                    pending.data.push_str(&data);
                    was_coalesced = true;
                })
                .or_insert(PendingTerminalOutput {
                    emit,
                    terminal_id,
                    data,
                    queued_at,
                });

            if was_coalesced {
                state.metrics.record_coalesced(EmitClass::TerminalOutput);
            }
        }
        SchedulerCommand::QueueToolUpdate {
            emit,
            causality_id,
            message_id,
            tool_call_id,
            status,
            result,
            tool_call,
            queued_at,
        } => {
            state.metrics.record_queued(
                EmitClass::ToolUpdate,
                estimate_tool_update_bytes(
                    &message_id,
                    &tool_call_id,
                    &status,
                    &result,
                    &tool_call,
                ),
            );
            let key = format!("{}\u{001f}{}", message_id, tool_call_id);
            if state.pending_tool_update.contains_key(&key) {
                state.metrics.record_coalesced(EmitClass::ToolUpdate);
            }
            state.pending_tool_update.insert(
                key,
                PendingToolUpdate {
                    emit,
                    causality_id,
                    message_id,
                    tool_call_id,
                    status,
                    result,
                    tool_call,
                    queued_at,
                },
            );
        }
        SchedulerCommand::QueueToolActivity {
            emit,
            tool_name,
            file_path,
            action,
            tool_call_id,
            queued_at,
        } => {
            state
                .metrics
                .record_queued(EmitClass::ToolActivity, file_path.len() + action.len());
            let key = format!(
                "{}\u{001f}{}\u{001f}{}\u{001f}{}",
                tool_name,
                file_path,
                action,
                tool_call_id.clone().unwrap_or_default()
            );
            if state.pending_tool_activity.contains_key(&key) {
                state.metrics.record_coalesced(EmitClass::ToolActivity);
            }
            state.pending_tool_activity.insert(
                key,
                PendingToolActivity {
                    emit,
                    tool_name,
                    file_path,
                    action,
                    tool_call_id,
                    queued_at,
                },
            );
        }
        SchedulerCommand::QueueExplorerRefresh { emit, queued_at } => {
            state.metrics.record_queued(EmitClass::ExplorerRefresh, 0);
            if state.pending_explorer_refresh.is_some() {
                state.metrics.record_coalesced(EmitClass::ExplorerRefresh);
            } else {
                state.pending_explorer_refresh = Some(PendingExplorerRefresh { emit, queued_at });
            }
        }
        SchedulerCommand::MetricsSnapshot { reply } => {
            let _ = reply.send(state.metrics.snapshot(state));
        }
    }
}

fn flush(state: &mut SchedulerState, reason: FlushReason) {
    if !has_pending(state) {
        return;
    }

    state.metrics.record_flush(reason);
    let mut flush_emit: Option<EmitFn> = None;
    let mut batch = Vec::new();

    while let Some(pending) = state.pending_chat.pop_front() {
        let seq_entry = state
            .chat_seq_by_message
            .entry(pending.message_id.clone())
            .or_insert(0);
        let seq = *seq_entry;
        *seq_entry += 1;
        let event = match pending.kind {
            ChatDeltaKind::Content => BladeEvent::Chat(ChatEvent::MessageDelta {
                id: pending.message_id,
                seq,
                chunk: pending.chunk,
                is_final: false,
            }),
            ChatDeltaKind::Reasoning => BladeEvent::Chat(ChatEvent::ReasoningDelta {
                id: pending.message_id,
                seq,
                chunk: pending.chunk,
                is_final: false,
            }),
        };

        let class = match pending.kind {
            ChatDeltaKind::Content => EmitClass::ChatMessageDelta,
            ChatDeltaKind::Reasoning => EmitClass::ChatReasoningDelta,
        };
        if flush_emit.is_none() {
            flush_emit = Some(pending.emit.clone());
        }
        batch.push(PendingTransportEnvelope {
            class,
            envelope: build_envelope(pending.causality_id, event),
            queued_at: pending.queued_at,
        });
    }

    let drained_terminal: Vec<_> = state
        .pending_terminal
        .drain()
        .map(|(_, pending)| pending)
        .collect();
    for pending in drained_terminal {
        let seq = state
            .terminal_seq
            .entry(pending.terminal_id.clone())
            .or_insert(0);
        let event = BladeEvent::Terminal(TerminalEvent::Output {
            id: pending.terminal_id,
            seq: *seq,
            data: pending.data,
        });
        *seq += 1;
        if flush_emit.is_none() {
            flush_emit = Some(pending.emit.clone());
        }
        batch.push(PendingTransportEnvelope {
            class: EmitClass::TerminalOutput,
            envelope: build_envelope(None, event),
            queued_at: pending.queued_at,
        });
    }

    let drained_tool_updates: Vec<_> = state
        .pending_tool_update
        .drain()
        .map(|(_, pending)| pending)
        .collect();
    for pending in drained_tool_updates {
        if flush_emit.is_none() {
            flush_emit = Some(pending.emit.clone());
        }
        batch.push(PendingTransportEnvelope {
            class: EmitClass::ToolUpdate,
            envelope: build_envelope(
                pending.causality_id,
                BladeEvent::Chat(ChatEvent::ToolUpdate {
                    message_id: pending.message_id,
                    tool_call_id: pending.tool_call_id,
                    status: pending.status,
                    result: pending.result,
                    tool_call: pending.tool_call,
                }),
            ),
            queued_at: pending.queued_at,
        });
    }

    let drained_tool_activity: Vec<_> = state
        .pending_tool_activity
        .drain()
        .map(|(_, pending)| pending)
        .collect();
    for pending in drained_tool_activity {
        if flush_emit.is_none() {
            flush_emit = Some(pending.emit.clone());
        }
        batch.push(PendingTransportEnvelope {
            class: EmitClass::ToolActivity,
            envelope: build_envelope(
                None,
                BladeEvent::Chat(ChatEvent::ToolActivity {
                    tool_name: pending.tool_name,
                    file_path: pending.file_path,
                    action: pending.action,
                    tool_call_id: pending.tool_call_id,
                }),
            ),
            queued_at: pending.queued_at,
        });
    }

    if let Some(emit) = flush_emit {
        emit_batch_tracked(state, &emit, batch);
    }

    if let Some(pending) = state.pending_explorer_refresh.take() {
        emit_refresh_tracked(
            state,
            &pending.emit,
            EmitClass::ExplorerRefresh,
            pending.queued_at,
        );
    }
}

pub fn emit_blade_event<R: Runtime>(
    app: &tauri::AppHandle<R>,
    causality_id: Option<String>,
    event: BladeEvent,
) {
    let _ = sender().send(SchedulerCommand::EmitEnvelope {
        emit: emitter_for(app),
        envelope: build_envelope(causality_id, event),
        queued_at: Instant::now(),
    });
}

pub fn emit_envelope<R: Runtime>(app: &tauri::AppHandle<R>, envelope: BladeEventEnvelope) {
    let _ = sender().send(SchedulerCommand::EmitEnvelope {
        emit: emitter_for(app),
        envelope,
        queued_at: Instant::now(),
    });
}

pub fn reset_chat_stream() {
    let _ = sender().send(SchedulerCommand::ResetChatStream);
}

pub fn queue_chat_message_delta<R: Runtime>(
    app: &tauri::AppHandle<R>,
    message_id: String,
    causality_id: Option<String>,
    chunk: String,
) {
    let _ = sender().send(SchedulerCommand::QueueChatDelta {
        emit: emitter_for(app),
        causality_id,
        message_id,
        kind: ChatDeltaKind::Content,
        chunk,
        queued_at: Instant::now(),
    });
}

pub fn queue_chat_reasoning_delta<R: Runtime>(
    app: &tauri::AppHandle<R>,
    message_id: String,
    causality_id: Option<String>,
    chunk: String,
) {
    let _ = sender().send(SchedulerCommand::QueueChatDelta {
        emit: emitter_for(app),
        causality_id,
        message_id,
        kind: ChatDeltaKind::Reasoning,
        chunk,
        queued_at: Instant::now(),
    });
}

pub fn queue_terminal_output<R: Runtime>(
    app: &tauri::AppHandle<R>,
    terminal_id: &str,
    data: String,
) {
    let _ = sender().send(SchedulerCommand::QueueTerminalOutput {
        emit: emitter_for(app),
        terminal_id: terminal_id.to_string(),
        data,
        queued_at: Instant::now(),
    });
}

pub fn queue_tool_update<R: Runtime>(
    app: &tauri::AppHandle<R>,
    causality_id: Option<String>,
    message_id: String,
    tool_call_id: String,
    status: String,
    result: Option<String>,
    tool_call: Option<ToolCall>,
) {
    let _ = sender().send(SchedulerCommand::QueueToolUpdate {
        emit: emitter_for(app),
        causality_id,
        message_id,
        tool_call_id,
        status,
        result,
        tool_call,
        queued_at: Instant::now(),
    });
}

pub fn queue_tool_activity(
    app: &tauri::AppHandle<impl Runtime>,
    tool_name: String,
    file_path: String,
    action: String,
    tool_call_id: Option<String>,
) {
    let _ = sender().send(SchedulerCommand::QueueToolActivity {
        emit: emitter_for(app),
        tool_name,
        file_path,
        action,
        tool_call_id,
        queued_at: Instant::now(),
    });
}

pub fn queue_refresh_explorer<R: Runtime>(app: &tauri::AppHandle<R>) {
    let _ = sender().send(SchedulerCommand::QueueExplorerRefresh {
        emit: refresh_emitter_for(app),
        queued_at: Instant::now(),
    });
}

pub fn metrics_snapshot() -> serde_json::Value {
    let (tx, rx) = mpsc::channel();
    if sender()
        .send(SchedulerCommand::MetricsSnapshot { reply: tx })
        .is_err()
    {
        return serde_json::json!({
            "available": false,
            "error": "scheduler unavailable",
        });
    }

    rx.recv_timeout(Duration::from_millis(250))
        .unwrap_or_else(|error| {
            serde_json::json!({
                "available": false,
                "error": format!("metrics snapshot failed: {}", error),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn immediate_event_does_not_overtake_queued_chat_delta() {
        let emitted_events = Arc::new(Mutex::new(Vec::<BladeEvent>::new()));
        let recorded_events = Arc::clone(&emitted_events);
        let emit: EmitFn = Arc::new(move |transport| {
            let mut events = recorded_events.lock().expect("event collector lock");
            match transport {
                BladeEventTransport::Single(envelope) => events.push(envelope.event),
                BladeEventTransport::Batch(envelopes) => {
                    events.extend(envelopes.into_iter().map(|envelope| envelope.event));
                }
            }
        });
        let mut state = SchedulerState::default();

        handle_command(
            &mut state,
            SchedulerCommand::QueueChatDelta {
                emit: Arc::clone(&emit),
                causality_id: Some("assistant-1".to_string()),
                message_id: "assistant-1".to_string(),
                kind: ChatDeltaKind::Content,
                chunk: "final words".to_string(),
                queued_at: Instant::now(),
            },
        );
        handle_command(
            &mut state,
            SchedulerCommand::EmitEnvelope {
                emit,
                envelope: build_envelope(
                    None,
                    BladeEvent::Chat(ChatEvent::MessageCompleted {
                        id: "assistant-1".to_string(),
                    }),
                ),
                queued_at: Instant::now(),
            },
        );

        let events = emitted_events.lock().expect("event collector lock");
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            BladeEvent::Chat(ChatEvent::MessageDelta { id, chunk, .. })
                if id == "assistant-1" && chunk == "final words"
        ));
        assert!(matches!(
            &events[1],
            BladeEvent::Chat(ChatEvent::MessageCompleted { id }) if id == "assistant-1"
        ));
    }
}
