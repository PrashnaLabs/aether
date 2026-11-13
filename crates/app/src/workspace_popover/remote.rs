use std::path::PathBuf;

use aether_git::{GitOp, https_url};
use aether_ui::{field, row_style};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, FocusHandle, Role, SharedString, Toggled, Window, div, prelude::*, px,
};

use super::model::{
    BAD_NAME, GhState, NO_COMMIT, NO_GH, RemoteRow, compare_url, footer_error, pr_branch,
    publish_args, refusal_reason, remote_row, repo_name_from, repo_name_ok, sync_spoken,
    sync_summary,
};
use super::rows::{icon, row, text};
use super::{PUBLISH_NAME_CONTEXT, PopConfirm, PopView, gh};
use crate::app::ChatApp;
use crate::git_panel::{Push, push_state, remote_ops};

impl ChatApp {
    pub(super) fn load_pop_remote(&mut self, cx: &mut Context<Self>) {
        if let Some(folder) = self.git_folder() {
            self.read_remotes(folder, cx);
        }
        if self.gh == Some(GhState::Ready) || self.workspace_pop.gh_checking {
            return;
        }
        self.workspace_pop.gh_checking = true;
        let task = cx.background_executor().spawn(async { gh::detect() });
        cx.spawn(async move |this, cx| {
            let state = task.await;
            this.update(cx, |this, cx| {
                this.workspace_pop.gh_checking = false;
                this.gh = Some(state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn read_remotes(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let task = self.agent.git_remotes(folder.clone());
        cx.spawn(async move |this, cx| {
            let remotes = task.await.ok();
            this.update(cx, |this, cx| {
                this.remotes.insert(folder, remotes);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pop_remote_state(&self) -> Result<RemoteRow, &'static str> {
        let read = self.git_folder().and_then(|f| self.remotes.get(&f));
        let remotes = match read {
            None => return Err("Reading remotes…"),
            Some(None) => return Err("Couldn't read remotes"),
            Some(Some(remotes)) => remotes,
        };
        let gh = self.gh.unwrap_or(GhState::Missing);
        Ok(remote_row(self.git_repo().is_some(), self.git_status(), remotes, gh))
    }

    fn pop_remote_web(&self) -> Option<(String, String)> {
        let Ok(RemoteRow::Remote { name, .. }) = self.pop_remote_state() else { return None };
        let remotes = self.remotes.get(&self.git_folder()?)?.as_ref()?;
        let https = https_url(&remotes.iter().find(|r| r.name == name)?.url)?;
        Some((name, https))
    }

    fn publish_off(&self) -> Option<&'static str> {
        match self.pop_remote_state() {
            Err(why) => Some(why),
            Ok(RemoteRow::Publish { blocked: Some(NO_GH) }) if self.gh.is_none() => {
                Some("Checking GitHub CLI…")
            }
            Ok(RemoteRow::Publish { blocked }) => blocked,
            Ok(_) => Some("This repository already has a remote"),
        }
    }

    fn init_repo(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.workspace_pop.error = None;
        let task = self.agent.git_init(folder.clone());
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.remotes.insert(folder.clone(), Some(Vec::new()));
                    }
                    Err(e) => this.pop_error(footer_error("Couldn't initialize git", &e), cx),
                }
                this.git.invalidate(&folder);
                this.branch_moved(folder, cx);
                this.refresh_git(false, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_publish_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.pop_where();
        let name = at
            .project
            .or(at.folder)
            .and_then(|p| p.file_name().map(|n| repo_name_from(&n.to_string_lossy())))
            .unwrap_or_default();
        self.workspace_pop.publish_name.update(cx, |s, cx| s.set_value(name, window, cx));
        self.workspace_pop.publish_private = true;
        self.pop_view(PopView::Publish, window, cx);
    }

    fn publish_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &self.workspace_pop;
        let name = pop.publish_name.read(cx).value().trim().to_string();
        let (Some(folder), Some(top)) =
            (self.git_folder(), self.git_status().map(|s| s.top.clone()))
        else {
            return;
        };
        if pop.publishing || name.is_empty() {
            return;
        }
        let bad_name = (!repo_name_ok(&name)).then_some(BAD_NAME);
        if let Some(why) = self.publish_off().or(bad_name) {
            self.workspace_pop.error = Some(why.into());
            return cx.notify();
        }
        let args = publish_args(&name, pop.publish_private, &top);
        self.workspace_pop.publishing = true;
        self.workspace_pop.error = None;
        let task = cx.background_executor().spawn(async move { gh::run(&top, &args, gh::SLOW) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.workspace_pop.publishing = false;
                // a failed push can still have made the repo and its origin
                this.read_remotes(folder, cx);
                this.refresh_git(false, cx);
                match result {
                    Err(e) => this.pop_error(e, cx),
                    Ok(()) => {
                        let pop = &this.workspace_pop;
                        if pop.view == PopView::Publish {
                            this.pop_view(PopView::Main, window, cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn pop_git(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_pop.error = None;
        let (Some(folder), Some(top)) =
            (self.git_folder(), self.git_status().map(|s| s.top.clone()))
        else {
            return;
        };
        let busy = self.git.busy();
        self.run_git_in(folder.clone(), top, op, None, window, cx);
        if busy || !self.git.busy() {
            let why = refusal_reason(busy, self.switching_folder(&folder));
            self.workspace_pop.error = Some(why.into());
        }
        cx.notify();
    }

    fn create_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_pop.error = None;
        let Some(status) = self.git_status() else { return };
        let (top, upstream, local) =
            (status.top.clone(), status.upstream.clone(), status.branch.clone());
        if self.gh == Some(GhState::Ready) {
            let args = ["pr", "create", "--web"].map(String::from).to_vec();
            let task =
                cx.background_executor().spawn(async move { gh::run(&top, &args, gh::QUICK) });
            cx.spawn(async move |this, cx| {
                let Err(e) = task.await else { return };
                this.update(cx, |this, cx| this.pop_error(e, cx)).ok();
            })
            .detach();
        } else if let Some((name, https)) = self.pop_remote_web()
            && let Some(branch) = pr_branch(upstream.as_deref(), &name, local.as_deref())
        {
            cx.open_url(&compare_url(&https, &branch));
            self.close_workspace_pop(window, cx);
        }
    }

    pub(super) fn pop_init_row(
        &mut self,
        handle: &FocusHandle,
        folder: PathBuf,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        row("pop-init", handle, "Initialize git repository".into(), None, cx)
            .child(icon(Lucide::GitBranchPlus, cx))
            .child(text("Initialize git repository", None, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.init_repo(folder.clone(), cx)))
            .into_any_element()
    }

    pub(super) fn pop_remote_row(
        &mut self,
        handle: &FocusHandle,
        off: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = match off {
            Some(why) => Err(why),
            None => self.pop_remote_state(),
        };
        let muted = cx.theme().muted_foreground;
        match state {
            Ok(RemoteRow::Publish { .. }) => {
                let off = self.publish_off();
                row("pop-remote", handle, "Publish repository".into(), off, cx)
                    .child(icon(Lucide::CloudUpload, cx))
                    .child(text("Publish repository", off.map(Into::into), cx))
                    .child(icon(Lucide::ChevronRight, cx))
                    .when(off.is_none(), |r| {
                        r.on_click(
                            cx.listener(|this, _, window, cx| this.open_publish_view(window, cx)),
                        )
                    })
                    .into_any_element()
            }
            Ok(RemoteRow::PublishBranch) => {
                let push = self.git_status().map(push_state);
                let busy = self.git.busy().then(|| refusal_reason(true, false));
                let (sub, off): (Option<SharedString>, _) = match push {
                    Some(Push::Ready { tip, .. }) => (Some(tip.into()), busy),
                    Some(Push::Blocked(why)) => (Some(why.into()), Some(why)),
                    _ if self.git_status().is_some_and(|s| s.commit.is_none()) => {
                        (Some(NO_COMMIT.into()), Some(NO_COMMIT))
                    }
                    _ => (None, busy),
                };
                let sub = busy.map(Into::into).or(sub);
                row("pop-remote", handle, "Publish branch".into(), off, cx)
                    .child(icon(Lucide::CloudUpload, cx))
                    .child(text("Publish branch", sub, cx))
                    .when(off.is_none(), |r| {
                        r.on_click(
                            cx.listener(|this, _, window, cx| {
                                this.pop_git(GitOp::Push, window, cx)
                            }),
                        )
                    })
                    .into_any_element()
            }
            Ok(RemoteRow::Remote { name, slug, ahead, behind, gone }) => {
                let title = match &slug {
                    Some(slug) => format!("{name} \u{b7} {slug}"),
                    None => name.clone(),
                };
                let counts = (!gone).then_some((ahead, behind));
                let spoken = format!("{}, {}", title, sync_spoken(counts));
                row("pop-remote", handle, spoken.into(), None, cx)
                    .child(icon(Lucide::Cloud, cx))
                    .child(text(title, None, cx))
                    .child(
                        div().flex_none().text_xs().text_color(muted).child(sync_summary(counts)),
                    )
                    .child(icon(Lucide::ChevronRight, cx))
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.pop_view(PopView::Remote, window, cx)
                        }),
                    )
                    .into_any_element()
            }
            Ok(RemoteRow::Init) | Err(_) => {
                let why = state.err().unwrap_or("Reading remotes…");
                row("pop-remote", handle, "Remote".into(), Some(why), cx)
                    .child(icon(Lucide::Cloud, cx))
                    .child(text("Remote", Some(why.into()), cx))
                    .into_any_element()
            }
        }
    }

    pub(super) fn pop_publish(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let h = self.workspace_pop.rows(3, cx)[1..].to_vec();
        let pop = &self.workspace_pop;
        let (private, publishing) = (pop.publish_private, pop.publishing);
        let name = pop.publish_name.read(cx).value().trim().to_string();
        let named = !name.is_empty();
        let off = self.publish_off().or((named && !repo_name_ok(&name)).then_some(BAD_NAME));
        let branch = self.git_status().and_then(|s| s.branch.clone());
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let note: SharedString = match (off, branch) {
            (Some(why), _) => why.into(),
            (None, Some(branch)) => format!("GitHub via gh \u{b7} pushes {branch}").into(),
            (None, None) => "GitHub via gh".into(),
        };
        let cards = [
            ("Private", "Only you and collaborators", true),
            ("Public", "Anyone on the internet can see it", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (title, sub, value))| {
            let selected = private == value;
            let base = h_flex()
                .id(("pop-visibility", i))
                .track_focus(&h[i])
                .tab_index(0)
                .role(Role::RadioButton)
                .aria_label(format!("{title}, {sub}"))
                .aria_toggled(if selected { Toggled::True } else { Toggled::False })
                .gap(px(8.))
                .items_start()
                .px(px(10.))
                .py(px(8.))
                .text_sm();
            let dot = if selected { Lucide::CircleDot } else { Lucide::Circle };
            row_style(base, selected, cx)
                .when(!selected, |c| c.border_color(border))
                .child(icon(dot, cx).mt(px(2.)))
                .child(
                    v_flex()
                        .child(div().font_medium().child(title))
                        .child(div().text_xs().text_color(muted).child(sub)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace_pop.publish_private = value;
                    cx.notify();
                }))
        });
        vec![
            v_flex()
                .gap(px(4.))
                .px(px(10.))
                .pb(px(4.))
                .key_context(PUBLISH_NAME_CONTEXT)
                .on_action(
                    cx.listener(|this, _: &PopConfirm, window, cx| this.publish_repo(window, cx)),
                )
                .child(div().text_xs().text_color(muted).child("Repository name"))
                .child(
                    field(&self.workspace_pop.publish_name).small().aria_label("Repository name"),
                )
                .into_any_element(),
            v_flex()
                .id("pop-visibility")
                .role(Role::RadioGroup)
                .aria_label("Visibility")
                .gap(px(4.))
                .children(cards)
                .into_any_element(),
            h_flex()
                .gap(px(8.))
                .items_center()
                .px(px(10.))
                .pt(px(6.))
                .pb(px(2.))
                .child(div().flex_1().min_w_0().text_xs().text_color(muted).child(note))
                .child(
                    aether_ui::button("pop-publish")
                        .primary()
                        .label("Publish")
                        .loading(publishing)
                        .disabled(off.is_some() || publishing || !named)
                        .on_click(cx.listener(|this, _, window, cx| this.publish_repo(window, cx))),
                )
                .into_any_element(),
        ]
    }

    pub(super) fn pop_remote(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let h = self.workspace_pop.rows(6, cx)[1..].to_vec();
        let status = self.git_status();
        let ops = status.map(remote_ops);
        let gone = status.is_some_and(|s| s.upstream.is_some() && s.ahead_behind.is_none());
        let (ahead, behind) = status.and_then(|s| s.ahead_behind).unwrap_or_default();
        let branch = status.and_then(|s| s.branch.clone());
        let https = self.pop_remote_web().map(|(_, https)| https);
        let busy = self.git.busy().then(|| refusal_reason(true, false));
        let muted = cx.theme().muted_foreground;
        let ops_row = [
            (
                GitOp::Push,
                "Push",
                Lucide::ArrowUp,
                Some((format!("\u{2191}{ahead}"), ahead, "push")),
            ),
            (
                GitOp::Pull,
                "Pull",
                Lucide::ArrowDown,
                Some((format!("\u{2193}{behind}"), behind, "pull")),
            ),
            (GitOp::Fetch, "Fetch", Lucide::RotateCw, None),
        ];
        let mut out: Vec<AnyElement> = ops_row
            .into_iter()
            .enumerate()
            .map(|(i, (op, label, glyph, count))| {
                let ok = ops.as_ref().is_some_and(|ops| ops.iter().any(|(o, ok)| *o == op && *ok));
                let gone_off = (gone && op == GitOp::Pull).then_some("Upstream gone");
                let off = busy.or(gone_off).or((!ok).then_some("Not available on this branch"));
                let count = count.filter(|_| !gone);
                let spoken = match &count {
                    Some((_, n, verb)) => format!("{label}, {n} to {verb}"),
                    None => label.to_string(),
                };
                row(("pop-remote-op", i), &h[i], spoken.into(), off, cx)
                    .child(icon(glyph, cx))
                    .child(text(label, off.map(Into::into), cx))
                    .children(
                        count.map(|(c, _, _)| {
                            div().flex_none().text_xs().text_color(muted).child(c)
                        }),
                    )
                    .when(off.is_none(), |r| {
                        r.on_click(cx.listener(move |this, _, window, cx| {
                            this.pop_git(op.clone(), window, cx)
                        }))
                    })
                    .into_any_element()
            })
            .collect();
        let no_web = "This remote has no web address";
        let pr_off = match (&branch, &https) {
            (None, _) => Some("Check out a branch to open a pull request"),
            (_, None) if self.gh != Some(GhState::Ready) => Some(no_web),
            _ => None,
        };
        out.push(
            row("pop-pr", &h[3], "Create pull request".into(), pr_off, cx)
                .child(icon(Lucide::GitPullRequest, cx))
                .child(text("Create pull request", pr_off.map(Into::into), cx))
                .when(pr_off.is_none(), |r| {
                    r.on_click(cx.listener(|this, _, window, cx| this.create_pr(window, cx)))
                })
                .into_any_element(),
        );
        let web_off = https.is_none().then_some(no_web);
        out.push(
            row("pop-web", &h[4], "Open on GitHub".into(), web_off, cx)
                .child(icon(Lucide::ExternalLink, cx))
                .child(text("Open on GitHub", web_off.map(Into::into), cx))
                .when_some(https, |r, url| {
                    r.on_click(cx.listener(move |this, _, window, cx| {
                        cx.open_url(&url);
                        this.close_workspace_pop(window, cx);
                    }))
                })
                .into_any_element(),
        );
        out
    }
}
