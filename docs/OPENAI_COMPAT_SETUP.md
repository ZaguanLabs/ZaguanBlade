# OpenAI-Compatible Server Setup Guide

This guide explains how to connect Zaguán Blade to OpenAI-compatible local AI servers.

## What are OpenAI-Compatible Servers?

OpenAI-compatible servers implement the OpenAI API specification, allowing you to run local language models with a familiar API. Popular options include:

- **llama.cpp server** - Lightweight C++ implementation
- **LocalAI** - Drop-in replacement for OpenAI API
- **vLLM** - High-performance inference server
- **text-generation-webui** - Web UI with OpenAI extension
- **Ollama** - Also provides OpenAI-compatible endpoints

## Key Features

✅ **No API Key Required** - These are local, keyless servers
✅ **Custom System Prompts** - Per-model prompt configuration
✅ **Streaming Support** - Real-time response streaming
✅ **Multiple Models** - Connect to any OpenAI-compatible server

## Setup Instructions

### 1. Start Your Local Server

Choose one of the following options:

#### Option A: llama.cpp server
```bash
# Build llama.cpp with server support
git clone https://github.com/ggerganov/llama.cpp
cd llama.cpp
make server

# Run the server (no API key!)
./server -m models/your-model.gguf --host 0.0.0.0 --port 8080
```

#### Option B: LocalAI
```bash
# Using Docker
docker run -p 8080:8080 -v $PWD/models:/models localai/localai:latest

# Or using binary
localai --models-path ./models --address :8080
```

#### Option C: vLLM
```bash
# Install vLLM
pip install vllm

# Run with OpenAI-compatible API
python -m vllm.entrypoints.openai.api_server \
  --model meta-llama/Llama-2-7b-hf \
  --port 8080
```

#### Option D: Ollama (OpenAI-compatible mode)
```bash
# Ollama also exposes OpenAI-compatible endpoints
# Just use: http://localhost:11434/v1
```

### 2. Configure Zaguán Blade

1. Open **Settings** (gear icon in top-right)
2. Navigate to **Local AI** tab
3. Find the **OpenAI-compatible Server** section
4. Toggle **Enable OpenAI-compatible** to ON
5. Enter your server URL:
   - Default: `http://localhost:8080/v1`
   - For Ollama: `http://localhost:11434/v1`
   - For remote servers: `http://your-server-ip:port/v1`
6. Click **Test Connection** to verify
7. Click **Refresh Models** to load available models
8. Click **Save Changes**

### 3. Select a Model

1. Look for the **Local Server** section in the model selector (bottom-left)
2. Your OpenAI-compatible models will appear there
3. Select a model to start chatting

## Custom System Prompts

You can configure custom system prompts for each model:

### Location
System prompts are stored in:
- **Linux/macOS**: `~/.config/zblade/prompts/`
- **Windows**: `%APPDATA%\zblade\prompts\`

### Creating a Prompt

1. Create a file named exactly as your model: `<model-name>.md`
   - Example: `llama-3-8b.md`
   - Example: `mistral-7b-instruct.md`

2. Write your system prompt in the file:
```markdown
You are a helpful AI assistant specialized in software development.

Current workspace: {{WORKSPACE_ROOT}}
Active file: {{ACTIVE_FILE}}
Operating system: {{OS}}
Shell: {{SHELL}}

Please provide concise, accurate responses focused on coding tasks.
```

### Template Variables

Available variables that will be replaced at runtime:
- `{{WORKSPACE_ROOT}}` - Current workspace path
- `{{ACTIVE_FILE}}` - Currently open file
- `{{OS}}` - Operating system (linux, macos, windows)
- `{{SHELL}}` - User's shell (bash, zsh, etc.)

## Troubleshooting

### Models Not Appearing

1. **Check server is running**:
   ```bash
   curl http://localhost:8080/v1/models
   ```
   Should return a JSON list of models.

2. **Verify URL format**:
   - Must end with `/v1`
   - Include `http://` or `https://`
   - Example: `http://localhost:8080/v1`

3. **Check firewall**:
   - Ensure port is not blocked
   - For remote servers, check network connectivity

4. **Click Refresh Models**:
   - Cache may be stale
   - Use the "Refresh Models" button in Settings

### Connection Test Fails

1. **Server not running**: Start your local server first
2. **Wrong port**: Check your server's port configuration
3. **Wrong URL**: Ensure URL ends with `/v1`
4. **Firewall**: Check if port is accessible

### Streaming Not Working

1. **Check server logs**: Look for errors in your server's output
2. **Model compatibility**: Some models may not support streaming
3. **Server configuration**: Ensure streaming is enabled in server config

### System Prompt Not Applied

1. **Check filename**: Must exactly match model name
2. **Check location**: Verify file is in correct prompts directory
3. **Check permissions**: Ensure file is readable
4. **Restart chat**: Start a new conversation to apply changes

## Server-Specific Notes

### llama.cpp
- Use `--ctx-size` to set context length
- Use `--n-gpu-layers` for GPU acceleration
- No API key flag needed

### LocalAI
- Supports multiple models simultaneously
- Configure models in `models.yaml`
- Automatic model downloading available

### vLLM
- Excellent performance for large models
- Supports tensor parallelism
- Use `--tensor-parallel-size` for multi-GPU

### Ollama
- Can use either Ollama-native or OpenAI-compatible endpoints
- OpenAI endpoint: `http://localhost:11434/v1`
- Native endpoint: `http://localhost:11434` (use Ollama section instead)

## Security Notes

⚠️ **Important Security Considerations**:

1. **Local Network Only**: By default, bind to `localhost` or `127.0.0.1`
2. **No Authentication**: These servers typically have no authentication
3. **Firewall**: Don't expose ports to the internet without proper security
4. **HTTPS**: Use HTTPS for remote connections when possible
5. **Trusted Networks**: Only connect to servers on trusted networks

## Performance Tips

1. **GPU Acceleration**: Use GPU layers for better performance
2. **Context Length**: Adjust based on your needs (longer = more memory)
3. **Batch Size**: Tune for your hardware
4. **Model Size**: Smaller models = faster responses
5. **Quantization**: Use quantized models (Q4, Q5) for better speed/memory

## Example Configurations

### Development Setup (Fast)
```bash
# llama.cpp with small quantized model
./server -m models/llama-3-8b-Q4_K_M.gguf \
  --ctx-size 4096 \
  --n-gpu-layers 35 \
  --port 8080
```

### Production Setup (Quality)
```bash
# vLLM with larger model
python -m vllm.entrypoints.openai.api_server \
  --model meta-llama/Llama-2-13b-hf \
  --tensor-parallel-size 2 \
  --port 8080
```

### Multi-Model Setup
```bash
# LocalAI with multiple models
docker run -p 8080:8080 \
  -v $PWD/models:/models \
  -v $PWD/config.yaml:/config.yaml \
  localai/localai:latest
```

## Getting Help

If you encounter issues:

1. Check server logs for errors
2. Verify server is accessible: `curl http://localhost:8080/v1/models`
3. Check Zaguán Blade logs for connection errors
4. Ensure no API key is being sent (these are keyless servers)
5. Try the "Test Connection" button in Settings

## Additional Resources

- [llama.cpp Documentation](https://github.com/ggerganov/llama.cpp)
- [LocalAI Documentation](https://localai.io/)
- [vLLM Documentation](https://docs.vllm.ai/)
- [OpenAI API Reference](https://platform.openai.com/docs/api-reference)
