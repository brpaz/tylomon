use anyhow::Result;
use crossterm::event::{Event, EventStream, MouseEventKind};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::app::Action;

pub fn spawn_input_task(tx: mpsc::Sender<Action>) -> tokio::task::JoinHandle<Result<()>> {
    tokio::spawn(async move {
        let mut event_stream = EventStream::new();

        while let Some(event_result) = event_stream.next().await {
            let event = event_result?;

            let action = match event {
                Event::Key(key) => Some(Action::KeyPress(key)),
                Event::Resize(w, h) => Some(Action::Resize(w, h)),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => Some(Action::MouseScroll {
                        column: mouse.column,
                        row: mouse.row,
                        delta: -3,
                    }),
                    MouseEventKind::ScrollDown => Some(Action::MouseScroll {
                        column: mouse.column,
                        row: mouse.row,
                        delta: 3,
                    }),
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        Some(Action::MouseClick {
                            column: mouse.column,
                            row: mouse.row,
                        })
                    }
                    _ => None,
                },
                _ => None,
            };

            if let Some(action) = action
                && tx.send(action).await.is_err()
            {
                break;
            }
        }

        Ok(())
    })
}
