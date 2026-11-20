//! [MEM-LIFETIME-EXAMPLE-OUTBOX]: while the editor is not reading stdout, every
//! workspace change must not queue another notification behind the pump.

use crate::wire::Outbox;

/// Workspace changes reported while the pump is stalled.
const CHANGES: usize = 32;

#[test]
fn workspace_changes_coalesce_while_the_editor_is_not_reading() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let outbox = Outbox::new(sender);
    for _ in 0..CHANGES {
        assert!(outbox.send_workspace_changed().is_ok(), "egress is open");
    }
    assert_eq!(
        receiver.len(),
        1,
        "{CHANGES} changes queued {} notifications behind a stalled pump",
        receiver.len()
    );
}
