# Attachments are stored as copied files, not inline or by reference

Every Attachment is copied into `<config dir>/attachments/<chat id>/` when it is added, and a message stores only that copy's path, name and mime type. We considered two alternatives. Storing image bytes as base64 inside the chat's messages JSON was rejected because the whole chat row is rewritten on every change, so each pasted screenshot would be re-serialized on every streamed update. Pointing at the user's original file was rejected because pasted images have no file at all, and picked files can move or vanish before a retry or an edit resends them.

## Consequences

- The attachment folder is deleted with its chat. On startup, folders whose chat no longer exists (unsent Drafts from a previous run) are removed.
- A fork copies the files into the new chat's folder, so deleting the original chat cannot break the fork.
- CLI agents receive these copies by path, so each driver must let its CLI read the attachments folder even though it sits outside the chat's Project.
