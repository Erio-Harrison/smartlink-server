# SmartLink WebSocket Server

这是 SmartLink 应用的 WebSocket 服务器，用于管理在线用户和转发实时消息。

## 功能

- 用户连接管理
- 实时消息转发
- 支持私聊和群聊消息
- 支持消息状态更新
- 支持 WebRTC 信令
- 支持输入状态指示

## 运行方式

### 本地开发

```bash
# 克隆仓库
git clone [repository-url]
cd smartlink-websocket-server

# 运行服务器
cargo run
```

服务器默认在 `0.0.0.0:8080` 监听 WebSocket 连接。

## 消息格式

服务器处理以下格式的 JSON 消息：

```json
{
  "message_type": "NewMessage",
  "sender_id": "user123",
  "conversation_id": "conv456",
  "recipient_id": "user789",
  "message_id": "msg123",
  "data": {
    "content": "Hello!",
    "content_type": "text",
    "recipients": ["user789"]
  },
  "timestamp": "2023-05-01T12:34:56.789Z"
}
```

## 连接方式

客户端连接后，必须首先发送一个 `UserStatus` 类型的消息来验证身份：

```json
{
  "message_type": "UserStatus",
  "sender_id": "user123",
  "data": {
    "status": "online"
  },
  "timestamp": "2023-05-01T12:34:56.789Z"
}
```

## 消息类型

服务器支持以下消息类型：

- `UserStatus`: 用户状态更新
- `NewMessage`: 新消息
- `MessageStatusUpdate`: 消息状态更新（已读、已送达等）
- `WebRTCSignal`: 用于 WebRTC 通话的信令
- `TypingIndicator`: 输入状态指示

## 配置选项

目前服务器使用硬编码的配置。未来版本将支持通过环境变量或配置文件进行配置。