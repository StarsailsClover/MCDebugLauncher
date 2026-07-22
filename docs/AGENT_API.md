# Agent API Guide

MCDebugLauncher includes a built-in HTTP/WebSocket server for programmatic control by AI agents and automation tools.

## Quick Start

Start the agent server:
```bash
mdl agent --port 8080 --bind 127.0.0.1
```

Server will start on `http://127.0.0.1:8080`.

## REST API

### GET /api/v1/status

Get server status and running instances.

**Response:**
```json
{
  "version": "0.1.0",
  "uptime": 3600,
  "active_instances": ["my-instance"],
  "running_instances": {
    "my-instance": {
      "pid": 12345,
      "started": "2026-07-22T10:00:00Z"
    }
  }
}
```

### POST /api/v1/execute

Execute a command programmatically.

**Request:**
```json
{
  "command": "list",
  "args": [],
  "options": {}
}
```

**Response:**
```json
{
  "status": "success",
  "exit_code": 0,
  "stdout": "Found 2 instances",
  "data": {
    "count": 2,
    "instances": [
      {
        "name": "test-instance",
        "version": "1.21.1",
        "loader": {
          "type": "fabric",
          "version": "0.16.0"
        }
      }
    ]
  }
}
```

**Supported Commands:**
- `info` - System information
- `versions` - List Minecraft versions
- `version-info` - Version details
- `list` - List instances
- `create` - Create instance
- `diagnose` - Diagnostics
- `logs` - View logs

## WebSocket Events

Connect to `ws://localhost:8080/api/v1/events` to receive real-time events.

### Event Types

#### Launch Events

**launch_started:**
```json
{
  "type": "launch_started",
  "instance": "my-instance",
  "timestamp": "2026-07-22T10:00:00Z"
}
```

**launch_progress:**
```json
{
  "type": "launch_progress",
  "instance": "my-instance",
  "stage": "downloading_libraries",
  "progress": 0.45,
  "message": "Downloaded 23/51 libraries",
  "timestamp": "2026-07-22T10:00:15Z"
}
```

**launch_completed:**
```json
{
  "type": "launch_completed",
  "instance": "my-instance",
  "pid": 12345,
  "timestamp": "2026-07-22T10:00:30Z"
}
```

**launch_failed:**
```json
{
  "type": "launch_failed",
  "instance": "my-instance",
  "error": "Failed to download library: connection timeout",
  "timestamp": "2026-07-22T10:00:20Z"
}
```

#### Log Events

**log_line:**
```json
{
  "type": "log_line",
  "instance": "my-instance",
  "level": "info",
  "message": "[Render thread/INFO]: Setting user: Player123",
  "timestamp": "2026-07-22T10:01:00Z"
}
```

Log levels: `debug`, `info`, `warn`, `error`

#### Instance Events

**instance_stopped:**
```json
{
  "type": "instance_stopped",
  "instance": "my-instance",
  "exit_code": 0,
  "timestamp": "2026-07-22T11:00:00Z"
}
```

## Client Examples

### Python

**HTTP Client:**
```python
import requests
import json

# Get status
response = requests.get("http://localhost:8080/api/v1/status")
status = response.json()
print(f"Server uptime: {status['uptime']}s")

# Execute command
payload = {
    "command": "list",
    "args": [],
    "options": {}
}
response = requests.post(
    "http://localhost:8080/api/v1/execute",
    json=payload
)
result = response.json()
print(f"Found {result['data']['count']} instances")
```

**WebSocket Client:**
```python
import asyncio
import websockets
import json

async def listen_events():
    uri = "ws://localhost:8080/api/v1/events"
    
    async with websockets.connect(uri) as websocket:
        print("Connected to event stream")
        
        async for message in websocket:
            event = json.loads(message)
            event_type = event.get('type')
            instance = event.get('instance')
            
            if event_type == 'launch_progress':
                progress = event.get('progress', 0) * 100
                message = event.get('message', '')
                print(f"[{instance}] {progress:.0f}% - {message}")
            
            elif event_type == 'launch_completed':
                pid = event.get('pid')
                print(f"[{instance}] Launch completed, PID: {pid}")
            
            elif event_type == 'log_line':
                level = event.get('level')
                msg = event.get('message')
                print(f"[{instance}] [{level}] {msg}")

if __name__ == "__main__":
    asyncio.run(listen_events())
```

### JavaScript/Node.js

**HTTP Client:**
```javascript
const fetch = require('node-fetch');

async function getStatus() {
  const response = await fetch('http://localhost:8080/api/v1/status');
  const status = await response.json();
  console.log(`Server uptime: ${status.uptime}s`);
}

async function executeCommand(command, args = [], options = {}) {
  const response = await fetch('http://localhost:8080/api/v1/execute', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args, options })
  });
  return await response.json();
}

// Usage
executeCommand('list').then(result => {
  console.log(`Found ${result.data.count} instances`);
});
```

**WebSocket Client:**
```javascript
const WebSocket = require('ws');

const ws = new WebSocket('ws://localhost:8080/api/v1/events');

ws.on('open', () => {
  console.log('Connected to event stream');
});

ws.on('message', (data) => {
  const event = JSON.parse(data);
  
  if (event.type === 'launch_progress') {
    const progress = (event.progress * 100).toFixed(0);
    console.log(`[${event.instance}] ${progress}% - ${event.message}`);
  }
  
  if (event.type === 'log_line') {
    console.log(`[${event.instance}] [${event.level}] ${event.message}`);
  }
});

ws.on('close', () => {
  console.log('Connection closed');
});
```

### cURL

**Get Status:**
```bash
curl http://localhost:8080/api/v1/status | jq .
```

**List Instances:**
```bash
curl -X POST http://localhost:8080/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{"command":"list","args":[],"options":{}}' | jq .
```

**Create Instance:**
```bash
curl -X POST http://localhost:8080/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{
    "command": "create",
    "args": ["test-instance"],
    "options": {
      "mc-version": "1.21.1",
      "loader": "fabric"
    }
  }' | jq .
```

## Use Cases

### AI Agent Automation

```python
import asyncio
import requests
import websockets
import json

class MinecraftAgent:
    def __init__(self, api_url="http://localhost:8080"):
        self.api_url = api_url
        self.ws_url = api_url.replace("http", "ws") + "/api/v1/events"
    
    def execute(self, command, args=[], options={}):
        """Execute a command and return structured result"""
        response = requests.post(
            f"{self.api_url}/api/v1/execute",
            json={"command": command, "args": args, "options": options}
        )
        return response.json()
    
    async def monitor_launch(self, instance_name):
        """Monitor instance launch with real-time progress"""
        async with websockets.connect(self.ws_url) as ws:
            async for message in ws:
                event = json.loads(message)
                
                if event.get('instance') != instance_name:
                    continue
                
                if event['type'] == 'launch_progress':
                    yield event['progress'], event['message']
                
                elif event['type'] == 'launch_completed':
                    yield 1.0, f"Launch completed (PID: {event['pid']})"
                    break
                
                elif event['type'] == 'launch_failed':
                    raise Exception(f"Launch failed: {event['error']}")

# Usage
agent = MinecraftAgent()

# Create instance
result = agent.execute("create", ["test"], {
    "mc-version": "1.21.1",
    "loader": "fabric"
})
print(f"Created instance: {result['data']['name']}")

# Monitor launch
async def launch_and_monitor():
    async for progress, message in agent.monitor_launch("test"):
        print(f"{progress*100:.0f}% - {message}")

asyncio.run(launch_and_monitor())
```

### Testing Framework Integration

```python
import pytest
import requests

class TestMinecraftLauncher:
    API_URL = "http://localhost:8080/api/v1"
    
    def execute(self, command, args=[], options={}):
        response = requests.post(
            f"{self.API_URL}/execute",
            json={"command": command, "args": args, "options": options}
        )
        return response.json()
    
    def test_create_instance(self):
        result = self.execute("create", ["pytest-test"], {
            "mc-version": "1.21.1",
            "loader": "fabric"
        })
        assert result['status'] == 'success'
        assert result['data']['name'] == 'pytest-test'
    
    def test_list_instances(self):
        result = self.execute("list")
        assert result['status'] == 'success'
        assert result['data']['count'] >= 0
    
    def test_diagnose(self):
        result = self.execute("diagnose", ["pytest-test"], {"analyze": "true"})
        assert result['status'] == 'success'
        assert 'issues' in result['data']
```

## Error Handling

All endpoints return structured error responses:

```json
{
  "status": "error",
  "exit_code": 1,
  "stdout": "Instance 'nonexistent' not found",
  "data": null
}
```

HTTP status codes:
- `200 OK` - Success
- `400 Bad Request` - Invalid command or arguments
- `500 Internal Server Error` - Server error

## Security Considerations

The agent server is designed for local development and testing:

- **Default binding**: `127.0.0.1` (localhost only)
- **No authentication**: Suitable for local use only
- **No TLS**: HTTP/WebSocket without encryption

For production use:
1. Use a reverse proxy (nginx, Caddy) for TLS
2. Implement authentication (API keys, OAuth)
3. Bind to specific network interfaces only
4. Use firewall rules to restrict access

## Performance

- **HTTP endpoints**: Low latency (<10ms typical)
- **WebSocket**: Real-time event delivery with <100ms latency
- **Concurrent connections**: Supports multiple WebSocket clients
- **Event buffering**: 1024 events per channel (configurable)

## Troubleshooting

### Server won't start

Check if port is already in use:
```bash
# Windows
netstat -ano | findstr :8080

# Linux/macOS
lsof -i :8080
```

Use a different port:
```bash
mdl agent --port 8081
```

### WebSocket connection fails

Verify the server is running:
```bash
curl http://localhost:8080/api/v1/status
```

Check WebSocket URL format:
- Correct: `ws://localhost:8080/api/v1/events`
- Wrong: `http://localhost:8080/api/v1/events`

### No events received

Events are only sent when actions occur. Try:
1. Execute a command via `/api/v1/execute`
2. Launch an instance in another terminal
3. Check server logs for errors

## See Also

- [Specification](specification.md) - Complete API reference
- [Research Document](RESEARCH.md) - Architecture and design decisions
- [README](../README.md) - Project overview
