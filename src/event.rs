use crate::action::Action;
use crate::error::AppResult;
use crossterm::event::{EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    Paste(String),
    Tick,
    BackgroundAction(Action),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    _tx: mpsc::UnboundedSender<Event>,
    _task: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        let task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = interval(Duration::from_millis(tick_rate_ms));

            loop {
                let tick_delay = tick_interval.tick();
                let crossterm_event = reader.next();

                tokio::select! {
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(crossterm::event::Event::Key(key))) => {
                                if key.kind == KeyEventKind::Press {
                                    let _ = event_tx.send(Event::Key(key));
                                }
                            }
                            Some(Ok(crossterm::event::Event::Paste(text))) => {
                                let _ = event_tx.send(Event::Paste(text));
                            }
                            Some(Err(_)) => break,
                            None => break,
                            _ => {}
                        }
                    }
                    _ = tick_delay => {
                        let _ = event_tx.send(Event::Tick);
                    }
                }
            }
        });

        Self {
            rx,
            _tx: tx,
            _task: task,
        }
    }

    pub async fn next(&mut self) -> AppResult<Event> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }
}
