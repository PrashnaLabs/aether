# Branch and worktree selector

## Approved behavior

Replace the composer's mixed Local/base-branch/shared-worktree menu with a searchable selector containing Branches and Worktrees tabs.

- Each branch offers explicit `Switch here` and `New worktree` actions.
- [x] Explicit user-requested follow-up: `Switch here` opens the existing warning alert naming the target branch and explaining that checkout affects other chats and tools using the same checkout. Cancel or dismissal changes nothing and starts no Git job. Only affirmative confirmation invokes the safe switch, rechecking guards for the captured initiating chat ID rather than the current active chat.
- Existing worktrees show their branch and folder. Selecting one uses that worktree for the draft.
- The composer trigger shows the current branch or selected worktree.
- Never force checkout, discard changes, or silently stash files. Show Git failures above the composer.
- Preserve existing uncommitted work, including the base-branch selector and worktree creation dialog.

## Implementation calls

Use Rust and existing GPUI Kit components, with no new dependencies. Keep the existing first-send creation and sharing behavior. Selection remains restricted to drafts, with actions disabled while an agent or Git operation is running.

Reuse the Git runner and blocking executor for branch switching. Use Git's worktree records rather than requiring a worktree to have an existing chat. Exclude missing worktrees and identify detached checkouts accurately.

Keyboard users must be able to search, change tabs, select a branch action, dismiss the popup, and return focus to the composer. Hover highlight must not replace the actual selected checkmark.
