# OpenAI-Compatible Server Setup

Zaguán Blade can list and chat with local servers that expose OpenAI-compatible `/v1/models` and chat-completions APIs. These models appear in the model picker under **Local Server** with IDs prefixed as `openai-compat/`.

OpenAI-compatible local servers are treated as keyless local providers. Blade does not send an API key to them.

## Supported Server Examples

Common choices include:

- `llama.cpp` server
- LocalAI
- vLLM
- text-generation-webui with its OpenAI extension
- Ollama's OpenAI-compatible endpoint

Use Blade's native Ollama section for normal Ollama usage. Use the OpenAI-compatible section only when you specifically want to talk to Ollama through its `/v1` compatibility API.

## Start a Local Server

### llama.cpp

```bash
git clone https://github.com/ggerganov/llama.cpp
cd llama.cpp
make server

./server -m models/your-model.gguf --host 127.0.0.1 --port 8080
```

### LocalAI

```bash
docker run -p 8080:8080 -v "$PWD/models:/models" localai/localai:latest
```

### vLLM

```bash
pip install vllm

python -m vllm.entrypoints.openai.api_server \
  --model meta-llama/Llama-2-7b-hf \
  --port 8080
```

### Ollama Compatibility Endpoint

```bash
ollama serve
```

Use `http://localhost:11434` in Blade. Blade normalizes the stored URL and appends `/v1/models` when listing OpenAI-compatible models.

## Configure Blade

1. Open **Settings**.
2. Go to **Local AI**.
3. Enable **OpenAI-compatible Server**.
4. Enter the server base URL.
   - Default local server: `http://localhost:8080`
   - Ollama compatibility endpoint: `http://localhost:11434`
   - Remote LAN server: `http://server-host:port`
5. Click **Test Connection**.
6. Click **Refresh Models**.
7. Save settings.

The UI placeholder may show a `/v1` URL. That is accepted, but Blade stores the normalized base URL without `/v1` and appends `/v1/models` internally.

## Select a Model

Open the chat model picker. OpenAI-compatible models appear under **Local Server**. Their runtime IDs use the form:

```text
openai-compat/<server-model-id>
```

## Custom System Prompts

Blade creates a global prompts directory on startup:

- Linux: `~/.config/zblade/prompts/`
- macOS: `~/Library/Application Support/com.zaguan.zblade/prompts/`
- Windows: `%APPDATA%\zaguan\zblade\config\prompts\` depending on the platform config directory returned by the OS

Create a Markdown file whose name matches the model. Blade tries several filename variants:

- the full model ID, for example `openai-compat.my-model.md`
- the stripped model name, for example `my-model.md`
- normalized slash/colon variants
- family matches, for example `qwen.md` for tagged Qwen variants

Example prompt:

```markdown
You are an AI coding assistant in Zaguán Blade.

Workspace root: {{WORKSPACE_ROOT}}
Active file: {{ACTIVE_FILE}}
Selected text or cursor context: {{SELECTION_OR_CURSOR}}
Operating system: {{OS}}
Shell: {{SHELL}}
Current date: {{CURRENT_DATE}}
Available tools: {{AVAILABLE_TOOLS}}
```

If no matching file exists, Blade uses its bundled local-AI system prompt.

Workspace `AGENTS.md` files are handled separately from these model prompts. When present, Blade adds the applicable repository instructions after the local
model prompt, including nested `AGENTS.md` files and local Markdown includes such as `@workflow.md`.

## Current Limitations

- Local OpenAI-compatible providers do not have built-in web fetch or deep research. Provide external context yourself when current web facts matter.
- Image attachments are disabled when only local models are available.
- Quality and tool-calling reliability depend heavily on the selected model and server implementation.
- Model lists are cached for about five minutes; use **Refresh Models** after changing the server.

## Troubleshooting

Check model listing directly:

```bash
curl http://localhost:8080/v1/models
```

Common fixes:

- Confirm the server is running and reachable from the Blade machine.
- Use a base URL Blade can normalize, such as `http://localhost:8080` or `http://localhost:8080/v1`.
- Check that the server actually implements `/v1/models`.
- Refresh models after enabling the provider.
- Review server logs if streaming or chat completion fails after model selection.

## Security Notes

Local OpenAI-compatible servers often run without authentication. Prefer binding them to `127.0.0.1` or a trusted private network. Do not expose keyless model servers to the public internet without adding your own authentication, firewalling, and TLS.
