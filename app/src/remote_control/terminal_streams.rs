use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use remote_control::limits::{OUTPUT_COALESCE_BYTES, OUTPUT_COALESCE_MS, OUTPUT_QUEUE_FRAMES};
use tokio::sync::mpsc;
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::{EntityId, ModelContext, ViewHandle};

use super::bridge::{ClientId, RemoteControlBridge};
use crate::terminal::view::TerminalView;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StreamFrame {
    Bytes(Arc<Vec<u8>>),
    Overflowed,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AttachId(pub u32);

struct Attachment {
    client_id: ClientId,
    terminal_id: EntityId,
    relay: SpawnedFutureHandle,
}

#[derive(Default)]
pub(crate) struct TerminalStreams {
    attachments: HashMap<u32, Attachment>,
    next_attach_id: u32,
}

impl TerminalStreams {
    pub fn attach(
        &mut self,
        client_id: ClientId,
        terminal: &ViewHandle<TerminalView>,
        sender: mpsc::Sender<(u32, StreamFrame)>,
        ctx: &mut ModelContext<RemoteControlBridge>,
    ) -> AttachId {
        self.next_attach_id = self.next_attach_id.wrapping_add(1).max(1);
        let attach_id = self.next_attach_id;
        let receiver = terminal.as_ref(ctx).subscribe_to_pty_reads(ctx);
        let relay = ctx.spawn(relay_pty_bytes(attach_id, receiver, sender), |_, _, _| {});
        self.attachments.insert(
            attach_id,
            Attachment {
                client_id,
                terminal_id: terminal.id(),
                relay,
            },
        );
        AttachId(attach_id)
    }

    pub fn detach(&mut self, attach_id: u32, client_id: ClientId) -> bool {
        let Some(attachment) = self.attachments.get(&attach_id) else {
            return false;
        };
        if attachment.client_id != client_id {
            return false;
        }
        if let Some(attachment) = self.attachments.remove(&attach_id) {
            attachment.relay.abort();
        }
        true
    }

    pub fn count_for_client(&self, client_id: ClientId) -> usize {
        self.attachments
            .values()
            .filter(|attachment| attachment.client_id == client_id)
            .count()
    }

    pub fn terminal_for(&self, attach_id: u32, client_id: ClientId) -> Option<EntityId> {
        self.attachments
            .get(&attach_id)
            .filter(|attachment| attachment.client_id == client_id)
            .map(|attachment| attachment.terminal_id)
    }

    pub fn detach_client(&mut self, client_id: ClientId) {
        let owned: Vec<u32> = self
            .attachments
            .iter()
            .filter(|(_, attachment)| attachment.client_id == client_id)
            .map(|(attach_id, _)| *attach_id)
            .collect();
        for attach_id in owned {
            if let Some(attachment) = self.attachments.remove(&attach_id) {
                attachment.relay.abort();
            }
        }
    }

    pub fn close_all(&mut self) {
        for (_, attachment) in self.attachments.drain() {
            attachment.relay.abort();
        }
    }
}

async fn relay_pty_bytes(
    attach_id: u32,
    receiver: Option<async_broadcast::Receiver<Arc<Vec<u8>>>>,
    sender: mpsc::Sender<(u32, StreamFrame)>,
) {
    let Some(mut receiver) = receiver else {
        let _ = sender.send((attach_id, StreamFrame::Closed)).await;
        return;
    };
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let received = receiver.recv().await;
        let Ok(bytes) = received else {
            let _ = sender.send((attach_id, StreamFrame::Closed)).await;
            return;
        };
        pending.extend_from_slice(&bytes);
        while pending.len() < OUTPUT_COALESCE_BYTES as usize {
            let deadline = Timer::after(Duration::from_millis(OUTPUT_COALESCE_MS));
            let next = futures::future::select(Box::pin(receiver.recv()), Box::pin(deadline)).await;
            match next {
                futures::future::Either::Left((Ok(more), _)) => pending.extend_from_slice(&more),
                futures::future::Either::Left((Err(_), _)) => {
                    if !pending.is_empty() {
                        let _ = send_or_overflow(&sender, attach_id, &mut pending).await;
                    }
                    let _ = sender.send((attach_id, StreamFrame::Closed)).await;
                    return;
                }
                futures::future::Either::Right((_, _)) => break,
            }
        }
        if send_or_overflow(&sender, attach_id, &mut pending)
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn send_or_overflow(
    sender: &mpsc::Sender<(u32, StreamFrame)>,
    attach_id: u32,
    pending: &mut Vec<u8>,
) -> Result<(), ()> {
    if pending.is_empty() {
        return Ok(());
    }
    let frame = Arc::new(std::mem::take(pending));
    match sender.try_send((attach_id, StreamFrame::Bytes(frame))) {
        Ok(()) => Ok(()),
        Err(mpsc::error::TrySendError::Full(_)) => sender
            .send((attach_id, StreamFrame::Overflowed))
            .await
            .map_err(|_| ()),
        Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
    }
}

pub(crate) fn output_queue_capacity() -> usize {
    OUTPUT_QUEUE_FRAMES as usize
}
