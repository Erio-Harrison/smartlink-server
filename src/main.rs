// websocket-server/src/main.rs
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info};

// 消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MessageType {
    UserStatus,
    NewMessage,
    MessageStatusUpdate,
    WebRTCSignal,
    TypingIndicator,
}

// WebSocket 消息结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSocketMessage {
    message_type: MessageType,
    sender_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recipient_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    timestamp: String,
}

// 用户连接映射：用户ID -> 发送通道
type UserConnections = HashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>;

// 服务器状态
struct ServerState {
    connections: RwLock<UserConnections>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            connections: RwLock::new(UserConnections::new()),
        }
    }

    // 添加用户连接
    async fn add_user(&self, user_id: String, sender: tokio::sync::mpsc::UnboundedSender<Message>) {
        let mut connections = self.connections.write().await;
        
        if connections.contains_key(&user_id) {
            info!("User {} reconnected, replacing existing connection", user_id);
        } else {
            info!("User {} connected", user_id);
        }
        
        connections.insert(user_id, sender);
    }

    // 移除用户连接
    async fn remove_user(&self, user_id: &str) {
        let mut connections = self.connections.write().await;
        if connections.remove(user_id).is_some() {
            info!("User {} disconnected", user_id);
        }
    }

    // 获取在线用户列表
    async fn get_online_users(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    // 向特定用户发送消息
    async fn send_to_user(&self, user_id: &str, message: &str) -> bool {
        let connections = self.connections.read().await;
        
        if let Some(sender) = connections.get(user_id) {
            match sender.send(Message::Text(message.to_string())) {
                Ok(_) => true,
                Err(e) => {
                    error!("Failed to send message to user {}: {}", user_id, e);
                    false
                }
            }
        } else {
            debug!("User {} is not online", user_id);
            false
        }
    }
}

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    // 创建服务器状态
    let state = Arc::new(ServerState::new());
    
    // 监听地址
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let addr_use_for_print = addr.clone();
    let listener = TcpListener::bind(addr).await.expect("Failed to bind to address");
    info!("WebSocket server listening on: {}", addr_use_for_print.clone());

    // 接受连接
    while let Ok((stream, addr)) = listener.accept().await {
        info!("Incoming connection from: {}", addr);
        
        // 为每个连接创建一个任务
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr, state).await {
                error!("Error handling connection: {}", e);
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    state: Arc<ServerState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_stream = accept_async(stream).await?;
    debug!("WebSocket connection established: {}", addr);
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    
    // 启动发送任务
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_sender.send(msg).await {
                error!("Error sending message: {}", e);
                break;
            }
        }
        debug!("Send task for {} terminated", addr);
    });
    
    // 等待第一条消息来识别用户
    let mut user_id = String::new();
    
    if let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
        match serde_json::from_str::<WebSocketMessage>(&text) {
            Ok(msg) => {
                if let MessageType::UserStatus = msg.message_type {
                    user_id = msg.sender_id.clone();
                    info!("User authenticated: {}", user_id);
                    state.add_user(user_id.clone(), tx.clone()).await;
                    
                    // 通知用户已连接成功
                    let response = serde_json::json!({
                        "message_type": "UserStatus",
                        "sender_id": "system",
                        "data": {
                            "status": "connected",
                            "message": "Successfully connected to server"
                        },
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }).to_string();
                    
                    tx.send(Message::Text(response))?;
                } else {
                    error!("First message must be UserStatus: {:?}", msg);
                    return Ok(());
                }
            }
            Err(e) => {
                error!("Failed to parse first message: {}", e);
                return Ok(());
            }
        }
    } else {
        error!("Failed to receive first message");
        return Ok(());
    }

    // 主消息处理循环
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                process_message(text, &user_id, &state).await?;
            }
            Ok(Message::Ping(data)) => {
                tx.send(Message::Pong(data))?;
            }
            Ok(Message::Pong(_)) => {
                // 忽略 Pong 响应
            }
            Ok(Message::Close(_)) => {
                info!("Client {} requested close", addr);
                break;
            }
            Ok(_) => {
                // 忽略其他类型的消息
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // 断开连接后清理
    state.remove_user(&user_id).await;
    info!("Connection closed: {}", addr);
    
    Ok(())
}

async fn process_message(
    text: String,
    sender_id: &str,
    state: &Arc<ServerState>,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Processing message from {}", sender_id);
    
    let message: WebSocketMessage = match serde_json::from_str(&text) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse message: {}", e);
            return Ok(());
        }
    };
    
    match message.message_type {
        MessageType::NewMessage => {
            // 处理新消息
            if let Some(recipient_id) = &message.recipient_id {
                // 直接消息
                state.send_to_user(recipient_id, &text).await;
            } else if let Some(conversation_id) = &message.conversation_id {
                // 群组消息 - 需要获取所有参与者并转发
                if let Some(data) = &message.data {
                    if let Some(recipients) = data.get("recipients").and_then(|r| r.as_array()) {
                        for recipient in recipients {
                            if let Some(recipient_id) = recipient.as_str() {
                                if recipient_id != sender_id {
                                    state.send_to_user(recipient_id, &text).await;
                                }
                            }
                        }
                    }
                }
            }
        }
        MessageType::MessageStatusUpdate => {
            // 处理消息状态更新
            if let Some(recipient_id) = &message.recipient_id {
                state.send_to_user(recipient_id, &text).await;
            }
        }
        MessageType::WebRTCSignal => {
            // 处理WebRTC信令
            if let Some(recipient_id) = &message.recipient_id {
                state.send_to_user(recipient_id, &text).await;
            }
        }
        MessageType::TypingIndicator => {
            // 处理输入状态指示器
            if let Some(recipient_id) = &message.recipient_id {
                state.send_to_user(recipient_id, &text).await;
            } else if let Some(data) = &message.data {
                // 群组输入指示
                if let Some(recipients) = data.get("recipients").and_then(|r| r.as_array()) {
                    for recipient in recipients {
                        if let Some(recipient_id) = recipient.as_str() {
                            if recipient_id != sender_id {
                                state.send_to_user(recipient_id, &text).await;
                            }
                        }
                    }
                }
            }
        }
        MessageType::UserStatus => {
            // 用户状态更新，可能是上线/下线通知
            // 我们可以将状态变更广播给好友，但这个简单实现中忽略
        }
    }
    
    Ok(())
}