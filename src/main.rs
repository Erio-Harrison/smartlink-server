// websocket-server/src/main.rs
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio::time;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MessageType {
    UserStatus,
    NewMessage,
    MessageStatusUpdate,
    WebRTCSignal,
    TypingIndicator,
    NewMoment,          
    MomentLike,          
    MomentComment,    
    MomentUpdate,     
    MomentDelete, 
}

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

struct UserConnection {
    sender: mpsc::UnboundedSender<Message>,
    last_ping: Instant,
}

type UserConnections = HashMap<String, UserConnection>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserOnlineStatus {
    user_id: String,
    status: String,
    timestamp: String,
}

struct ServerState {
    connections: RwLock<UserConnections>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            connections: RwLock::new(UserConnections::new()),
        }
    }

    async fn add_user(&self, user_id: String, sender: mpsc::UnboundedSender<Message>) {
        let mut connections = self.connections.write().await;
        
        let was_online = connections.contains_key(&user_id);
        
        connections.insert(user_id.clone(), UserConnection {
            sender,
            last_ping: Instant::now(),
        });
        
        if was_online {
            info!("User {} reconnected, replacing existing connection", user_id);
        } else {
            info!("User {} connected", user_id);
            drop(connections);
            self.broadcast_user_status(&user_id, "online").await;
        }
    }

    async fn remove_user(&self, user_id: &str) {
        let mut connections = self.connections.write().await;
        if connections.remove(user_id).is_some() {
            info!("User {} disconnected", user_id);
            drop(connections); 
            self.broadcast_user_status(user_id, "offline").await;
        }
    }

    async fn update_ping(&self, user_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(conn) = connections.get_mut(user_id) {
            conn.last_ping = Instant::now();
        }
    }

    async fn get_online_users(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    async fn send_to_user(&self, user_id: &str, message: &str) -> bool {
        let connections = self.connections.read().await;
        
        if let Some(conn) = connections.get(user_id) {
            match conn.sender.send(Message::Text(message.to_string())) {
                Ok(_) => {
                    debug!("Message sent to user {}", user_id);
                    true
                },
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

    async fn broadcast_user_status(&self, user_id: &str, status: &str) {
        let status_message = UserOnlineStatus {
            user_id: user_id.to_string(),
            status: status.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        
        let json_message = serde_json::to_string(&status_message).unwrap_or_default();
        
        // 这里简化为向所有人广播
        let online_users = self.get_online_users().await;
        for online_user in online_users {
            if online_user != user_id {
                self.send_to_user(&online_user, &json_message).await;
            }
        }
    }

    async fn cleanup_inactive_connections(&self) {
        let mut to_remove = Vec::new();
        
        {
            let connections = self.connections.read().await;
            let now = Instant::now();
            for (user_id, conn) in connections.iter() {
                if now.duration_since(conn.last_ping) > Duration::from_secs(60) {
                    warn!("User {} connection timed out", user_id);
                    to_remove.push(user_id.clone());
                }
            }
        }
        
        for user_id in to_remove {
            self.remove_user(&user_id).await;
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let state = Arc::new(ServerState::new());
    
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            cleanup_state.cleanup_inactive_connections().await;
        }
    });

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let addr_use_for_print = addr.clone();
    let listener = TcpListener::bind(addr).await.expect("Failed to bind to address");
    info!("SmartLink WebSocket server listening on: {}", addr_use_for_print);

    while let Ok((stream, addr)) = listener.accept().await {
        info!("Incoming connection from: {}", addr);
        
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
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_sender.send(msg).await {
                error!("Error sending message: {}", e);
                break;
            }
        }
        debug!("Send task for {} terminated", addr);
    });
    
    let mut user_id = String::new();
    
    if let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
        match serde_json::from_str::<WebSocketMessage>(&text) {
            Ok(msg) => {
                if let MessageType::UserStatus = msg.message_type {
                    user_id = msg.sender_id.clone();
                    info!("User authenticated: {}", user_id);
                    state.add_user(user_id.clone(), tx.clone()).await;
                    
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

    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                state.update_ping(&user_id).await;
                
                process_message(text, &user_id, &state).await?;
            }
            Ok(Message::Ping(data)) => {
                state.update_ping(&user_id).await;
                
                tx.send(Message::Pong(data))?;
            }
            Ok(Message::Pong(_)) => {
                state.update_ping(&user_id).await;
            }
            Ok(Message::Close(_)) => {
                info!("Client {} requested close", addr);
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

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
            info!("New message from {}", sender_id);
            
            if let Some(recipient_id) = &message.recipient_id {
                state.send_to_user(recipient_id, &text).await;
            } else if let Some(conversation_id) = &message.conversation_id {
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
            if let Some(recipient_id) = &message.recipient_id {
                info!("Message status update for message: {:?}", message.message_id);
                state.send_to_user(recipient_id, &text).await;
            }
        }
        MessageType::WebRTCSignal => {
            if let Some(recipient_id) = &message.recipient_id {
                info!("WebRTC signal from {} to {}", sender_id, recipient_id);
                state.send_to_user(recipient_id, &text).await;
            }
        }
        MessageType::TypingIndicator => {
            if let Some(recipient_id) = &message.recipient_id {
                debug!("Typing indicator from {} to {}", sender_id, recipient_id);
                state.send_to_user(recipient_id, &text).await;
            } else if let Some(conversation_id) = &message.conversation_id {
                debug!("Group typing indicator from {} in {}", sender_id, conversation_id);
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
        MessageType::UserStatus => {
            if let Some(data) = &message.data {
                if let Some(status) = data.get("status").and_then(|s| s.as_str()) {
                    info!("User {} status changed to {}", sender_id, status);
                }
            }
        }

        MessageType::NewMoment => {
            info!("New moment published by {}", sender_id);
            
                        if let Some(data) = &message.data {
                if let Some(friends) = data.get("friends").and_then(|f| f.as_array()) {
                    for friend in friends {
                        if let Some(friend_id) = friend.as_str() {
                            if friend_id != sender_id {
                                state.send_to_user(friend_id, &text).await;
                            }
                        }
                    }
                }
            }
        },
        MessageType::MomentLike | MessageType::MomentComment => {
            info!("Moment interaction from {}: {:?}", sender_id, message.message_type);
            
            if let Some(recipient_id) = &message.recipient_id {
                if recipient_id != sender_id {
                    state.send_to_user(recipient_id, &text).await;
                }
            }
        },
        MessageType::MomentUpdate | MessageType::MomentDelete => {
            info!("Moment updated/deleted by {}", sender_id);
            
            if let Some(data) = &message.data {
                if let Some(viewers) = data.get("viewers").and_then(|v| v.as_array()) {
                    for viewer in viewers {
                        if let Some(viewer_id) = viewer.as_str() {
                            if viewer_id != sender_id {
                                state.send_to_user(viewer_id, &text).await;
                            }
                        }
                    }
                }
            }
        },
    }
    
    Ok(())
}