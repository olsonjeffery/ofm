use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::bus::BroadcastBus;
use super::message::{ClientMessage, ServerMessage, WsTopic, WsTopicKind};
use crate::services;

pub async fn handle_socket(
    socket: WebSocket,
    bus: std::sync::Arc<BroadcastBus>,
    user_id: Uuid,
    db: hiqlite::Client,
) {
    let (mut sender, mut receiver) = socket.split();

    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let write_task = tokio::spawn(async move {
        while let Some(msg) = msg_rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let mut subscriptions: HashMap<WsTopic, tokio::task::JoinHandle<()>> = HashMap::new();

    while let Some(msg_result) = receiver.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                let client_msg: ClientMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        send_json(
                            &msg_tx,
                            &ServerMessage::Error {
                                message: format!("parse error: {}", e),
                            },
                        )
                        .await;
                        continue;
                    }
                };

                match client_msg {
                    ClientMessage::Subscribe { topics, since } => {
                        let mut subscribed = Vec::new();

                        for topic in topics {
                            let authorized = match topic.kind {
                                WsTopicKind::Project => {
                                    services::projects::get_project(&db, topic.id.0)
                                        .await
                                        .is_ok_and(|p| p.user_id == user_id)
                                }
                                WsTopicKind::Task => services::tasks::get_task(&db, topic.id.0)
                                    .await
                                    .is_ok_and(|t| t.user_id == user_id),
                            };
                            if !authorized {
                                send_json(
                                    &msg_tx,
                                    &ServerMessage::Error {
                                        message: format!(
                                            "not authorized for topic {:?}",
                                            topic.kind
                                        ),
                                    },
                                )
                                .await;
                                continue;
                            }

                            if let Some(since) = since {
                                let events = bus.events_since(&topic, since).await;
                                if !events.is_empty() {
                                    send_json(
                                        &msg_tx,
                                        &ServerMessage::EventsReplay {
                                            events,
                                            timestamp: Utc::now(),
                                        },
                                    )
                                    .await;
                                }
                            }

                            let rx = bus.subscribe(&topic).await;
                            subscribed.push(topic.clone());

                            let tx2 = msg_tx.clone();
                            let mut rx2 = rx;

                            let handle = tokio::spawn(async move {
                                loop {
                                    match rx2.recv().await {
                                        Ok(msg) => {
                                            if let Ok(json) = serde_json::to_string(&*msg) {
                                                if tx2.send(json).is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                        Err(broadcast::error::RecvError::Closed) => break,
                                        Err(broadcast::error::RecvError::Lagged(n)) => {
                                            let err = ServerMessage::Error {
                                                message: format!("missed {} events", n),
                                            };
                                            if let Ok(json) = serde_json::to_string(&err) {
                                                let _ = tx2.send(json);
                                            }
                                        }
                                    }
                                }
                            });

                            if let Some(old) = subscriptions.insert(topic.clone(), handle) {
                                old.abort();
                            }
                        }

                        send_json(&msg_tx, &ServerMessage::Subscribed { topics: subscribed }).await;
                    }

                    ClientMessage::Unsubscribe { topics } => {
                        for topic in &topics {
                            if let Some(handle) = subscriptions.remove(topic) {
                                handle.abort();
                            }
                        }
                        send_json(&msg_tx, &ServerMessage::Unsubscribed { topics }).await;
                    }

                    ClientMessage::Ping => {
                        send_json(&msg_tx, &ServerMessage::Pong).await;
                    }
                }
            }
            Ok(Message::Ping(_)) => {
                send_json(&msg_tx, &ServerMessage::Pong).await;
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::warn!("websocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    for (_, handle) in subscriptions.drain() {
        handle.abort();
    }

    let _ = write_task.await;
}

async fn send_json(tx: &tokio::sync::mpsc::UnboundedSender<String>, msg: &ServerMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = tx.send(json);
    }
}
