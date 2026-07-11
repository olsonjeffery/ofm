use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, Mutex};

use super::message::{ServerMessage, WsTopic};

const MAX_RECENT_EVENTS: usize = 500;

struct BroadcastChannel {
    sender: broadcast::Sender<Arc<ServerMessage>>,
    recent_events: VecDeque<Arc<ServerMessage>>,
}

impl BroadcastChannel {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            sender,
            recent_events: VecDeque::with_capacity(MAX_RECENT_EVENTS),
        }
    }

    fn push_event(&mut self, msg: Arc<ServerMessage>) {
        if self.recent_events.len() >= MAX_RECENT_EVENTS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(msg);
    }
}

pub struct BroadcastBus {
    channels: Mutex<HashMap<WsTopic, BroadcastChannel>>,
}

impl BroadcastBus {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            channels: Mutex::new(HashMap::new()),
        })
    }

    pub async fn subscribe(&self, topic: &WsTopic) -> broadcast::Receiver<Arc<ServerMessage>> {
        let mut channels = self.channels.lock().await;
        let channel = channels
            .entry(topic.clone())
            .or_insert_with(BroadcastChannel::new);
        channel.sender.subscribe()
    }

    pub async fn unsubscribe(&self, topic: &WsTopic) {
        let mut channels = self.channels.lock().await;
        if let Some(channel) = channels.get(topic) {
            if channel.sender.receiver_count() == 0 {
                channels.remove(topic);
            }
        }
    }

    pub async fn broadcast(&self, topic: &WsTopic, message: ServerMessage) {
        let msg = Arc::new(message);
        let mut channels = self.channels.lock().await;
        let channel = channels
            .entry(topic.clone())
            .or_insert_with(BroadcastChannel::new);
        channel.push_event(msg.clone());
        let _ = channel.sender.send(msg);
    }

    pub async fn events_since(&self, topic: &WsTopic, since: DateTime<Utc>) -> Vec<ServerMessage> {
        let channels = self.channels.lock().await;
        let Some(channel) = channels.get(topic) else {
            return Vec::new();
        };
        channel
            .recent_events
            .iter()
            .filter_map(|event| {
                event.timestamp().and_then(|ts| {
                    if ts > since {
                        Some((**event).clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub async fn cleanup_empty_topics(&self) {
        let mut channels = self.channels.lock().await;
        channels.retain(|_, channel| channel.sender.receiver_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Duration;

    fn test_topic() -> WsTopic {
        WsTopic {
            kind: super::super::message::WsTopicKind::Task,
            id: super::super::message::TopicId(42),
        }
    }

    fn test_event(topic: &WsTopic, timestamp: DateTime<Utc>) -> ServerMessage {
        ServerMessage::Event {
            topic: topic.clone(),
            event_type: "test".to_string(),
            timestamp,
            payload: serde_json::json!({"msg": "hello"}),
        }
    }

    #[tokio::test]
    async fn test_subscribe_broadcast_receive() {
        let bus = BroadcastBus::new();
        let topic = test_topic();

        let mut rx = bus.subscribe(&topic).await;

        let event = test_event(&topic, Utc::now());
        bus.broadcast(&topic, event.clone()).await;

        let received = rx.recv().await.unwrap();
        match &*received {
            ServerMessage::Event { event_type, .. } => {
                assert_eq!(event_type, "test");
            }
            _ => panic!("expected Event"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_multiple_receivers() {
        let bus = BroadcastBus::new();
        let topic = test_topic();

        let mut rx1 = bus.subscribe(&topic).await;
        let mut rx2 = bus.subscribe(&topic).await;

        let event = test_event(&topic, Utc::now());
        bus.broadcast(&topic, event).await;

        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();
        match &*received1 {
            ServerMessage::Event { event_type, .. } => {
                assert_eq!(event_type, "test");
            }
            _ => panic!("expected Event"),
        }
        match &*received2 {
            ServerMessage::Event { event_type, .. } => {
                assert_eq!(event_type, "test");
            }
            _ => panic!("expected Event"),
        }
    }

    #[tokio::test]
    async fn test_events_since_returns_matching() {
        let bus = BroadcastBus::new();
        let topic = test_topic();

        let t0 = Utc::now() - Duration::seconds(10);
        let t1 = Utc::now() - Duration::seconds(5);
        let t2 = Utc::now();

        bus.broadcast(&topic, test_event(&topic, t0)).await;
        bus.broadcast(&topic, test_event(&topic, t1)).await;
        bus.broadcast(&topic, test_event(&topic, t2)).await;

        let events = bus.events_since(&topic, t1).await;
        assert_eq!(events.len(), 1, "should return only event after t1");

        let events_all = bus.events_since(&topic, t0 - Duration::seconds(1)).await;
        assert_eq!(events_all.len(), 3, "should return all events");
    }

    #[tokio::test]
    async fn test_events_since_empty_topic() {
        let bus = BroadcastBus::new();
        let topic = test_topic();
        let events = bus.events_since(&topic, Utc::now()).await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_ring_buffer_max_size() {
        let bus = BroadcastBus::new();
        let topic = test_topic();

        for i in 0..MAX_RECENT_EVENTS + 50 {
            let ts = Utc::now() + Duration::seconds(i as i64);
            bus.broadcast(&topic, test_event(&topic, ts)).await;
        }

        let all = bus
            .events_since(&topic, Utc::now() - Duration::days(1))
            .await;
        assert!(all.len() <= MAX_RECENT_EVENTS);
        assert_eq!(all.len(), MAX_RECENT_EVENTS);
    }

    #[tokio::test]
    async fn test_multiple_topics_independent() {
        let bus = BroadcastBus::new();
        let topic_a = test_topic();
        let topic_b = {
            let mut t = topic_a.clone();
            t.id = super::super::message::TopicId(99);
            t
        };

        let mut rx_a = bus.subscribe(&topic_a).await;
        let mut rx_b = bus.subscribe(&topic_b).await;

        let event_a = test_event(&topic_a, Utc::now());
        bus.broadcast(&topic_a, event_a).await;

        let event_b = test_event(&topic_b, Utc::now());
        bus.broadcast(&topic_b, event_b).await;

        assert!(rx_a.recv().await.is_ok());
        assert!(rx_b.recv().await.is_ok());
    }
}
