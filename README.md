# SmartLink WebSocket Server

SmartLink WebSocket Server 是一个轻量级服务器，为 SmartLink 聊天应用提供实时通信支持。该服务器专注于连接管理和消息转发，而不处理认证和数据持久化（这些由客户端处理）。

## 功能特点

- 用户连接管理：跟踪在线用户并管理其 WebSocket 连接
- 实时消息通知：将消息通知转发给相关用户
- WebRTC 信令支持：为音视频通话提供信令传递
- 群组消息支持：支持群组聊天的消息转发

## 技术栈

- Rust
- Tokio (异步运行时)
- tokio-tungstenite (WebSocket 库)
- serde (JSON 序列化/反序列化)

## 消息类型

服务器支持以下消息类型：

- `UserStatus` - 用户状态更新（在线/离线）
- `NewMessage` - 新消息通知
- `MessageStatusUpdate` - 消息状态更新（已读/已送达）
- `WebRTCSignal` - WebRTC 信令（offer/answer/ice candidate）
- `TypingIndicator` - 用户输入状态
- `ConversationUpdated` - 会话信息更新
- `GroupMemberAdded` - 群组新增成员
- `GroupMemberRemoved` - 群组移除成员
- `SystemNotification` - 系统通知

## 消息格式

所有消息使用 JSON 格式，结构如下：

```json
{
  "messageType": "NewMessage",
  "senderId": "user123",
  "conversationId": "conv456",
  "recipientId": "user789",
  "messageId": "msg123",
  "data": {
    "recipients": ["user456", "user789"],
    "content": "Hello!",
    "timestamp": "2023-11-22T12:00:00Z"
  },
  "timestamp": "2023-11-22T12:00:00Z"
}
```

## 手动构建运行

```bash
# 克隆仓库
git clone https://github.com/yourusername/smartlink-websocket-server.git
cd smartlink-websocket-server

# 构建项目
cargo build --release

# 运行服务器
./target/release/smartlink-websocket-server
```

## 与客户端集成

SmartLink WebSocket 服务器设计为与 Tauri 客户端中的 `WebSocketClient` 配合使用。客户端通过以下流程与服务器交互：

1. 连接建立
   - 客户端连接到 `ws://server:8080`
   - 连接成功后发送 `UserStatus` 消息进行身份识别

2. 消息发送
   - 客户端先将消息保存到 MongoDB
   - 然后发送通知消息到 WebSocket 服务器
   - 服务器将通知转发给在线的接收者

3. 消息接收
   - 客户端收到通知后，从 MongoDB 获取完整消息内容
   - 客户端更新 UI 显示消息

4. 音视频通话
   - 通过 WebSocket 服务器交换 WebRTC 信令
   - 建立 P2P 连接后，媒体流直接在客户端之间传输

## 配置项

默认情况下，服务器监听 `0.0.0.0:8080`。如需修改，请在代码中更改 `addr` 变量或通过环境变量配置（未实现）。

## 注意事项

1. **认证**：服务器不验证用户身份，依赖客户端正确实现认证
2. **消息内容**：服务器只转发通知，不包含完整消息内容
3. **连接限制**：当前实现中，同一用户 ID 只允许一个活跃连接

## 扩展与优化

- 添加环境变量配置
- 实现消息发送者映射的过期机制
- 添加性能监控和日志记录
- 使用 Redis 替代内存存储用户连接
- 添加简单的负载均衡支持