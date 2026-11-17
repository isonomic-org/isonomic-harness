// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Presets, artifacts, activity, and the engine-only calls: recovery,
//! client connection changes and shutdown.

use crate::exec::lock;
use crate::host::{Host, Slot};
use crate::types::*;
use harness_core::future::BoxFuture;
use harness_core::*;
use harness_loop::Input as LoopInput;
use harness_preset::{parse, Diagnostic, Severity};
use std::sync::Arc;

type Fut<'a, T> = BoxFuture<'a, Result<T, HostError>>;

impl Host {
    /// Built-in presets and the caller's own (the shell sees every app's).
    pub fn list_presets(&self, caller: &Caller) -> Fut<'_, Vec<PresetSummary>> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move {
            let st = lock(&me.state);
            Ok(st
                .presets
                .iter()
                .filter(|((owner, _), _)| owner.as_ref().is_none_or(|o| caller.scope.sees(o)))
                .filter_map(|(_, versions)| versions.last())
                .map(|p| PresetSummary {
                    reference: p.reference.clone(),
                    owner: p.owner.clone(),
                    name: p
                        .preset
                        .as_ref()
                        .map_or_else(|| p.reference.id.to_string(), |x| x.name.clone()),
                    diagnostics: p.diagnostics.clone(),
                })
                .collect())
        })
    }

    /// A preset's latest version: the caller's own first, then built-in.
    pub fn get_preset(&self, caller: &Caller, id: &PresetId) -> Fut<'_, StoredPreset> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let st = lock(&me.state);
            let own = caller
                .scope
                .app()
                .and_then(|a| st.presets.get(&(Some(a.clone()), id.clone())));
            own.or_else(|| st.presets.get(&(None, id.clone())))
                .and_then(|v| v.last().cloned())
                .ok_or(HostError::NotFound)
        })
    }

    /// Validate and store a new version of one of the caller's presets.
    /// Limits and autonomy may only tighten the host's ceiling. An invalid
    /// preset is stored (and listed) with its diagnostics.
    pub fn put_preset(&self, caller: &Caller, toml: String) -> Fut<'_, PresetRef> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move {
            let Scope::App { app, .. } = &caller.scope else {
                return Err(HostError::Unauthorized);
            };
            let parsed = parse(&toml);
            let mut diagnostics = parsed.diagnostics.clone();
            let mut preset = parsed.preset.clone();
            if let Some(p) = &preset {
                diagnostics.extend(p.check_ceiling(&me.cfg.ceiling));
            }
            let id = preset
                .as_ref()
                .map(|p| p.reference.id.clone())
                .or_else(|| toml_id(&toml))
                .ok_or_else(|| HostError::InvalidPreset(diagnostics.clone()))?;
            let mut st = lock(&me.state);
            let versions = st
                .presets
                .entry((Some(app.clone()), id.clone()))
                .or_default();
            let latest = versions.last().map_or(0, |v| v.reference.version);
            let version = preset.as_ref().map_or(latest + 1, |p| p.reference.version);
            if version <= latest {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    path: "version".into(),
                    message: format!("must be greater than the stored version {latest}"),
                });
            }
            let failed = diagnostics.iter().any(|d| d.severity == Severity::Error);
            if failed {
                preset = None;
            }
            let reference = PresetRef {
                id,
                version: version.max(latest + 1),
            };
            versions.push(StoredPreset {
                reference: reference.clone(),
                owner: Some(app.clone()),
                text: toml,
                preset,
                diagnostics: diagnostics.clone(),
            });
            if failed {
                Err(HostError::InvalidPreset(diagnostics))
            } else {
                Ok(reference)
            }
        })
    }

    /// An artifact version (the latest when `version` is `None`): metadata
    /// from the log, body from the blob store.
    pub fn get_artifact(
        &self,
        caller: &Caller,
        artifact: &ArtifactId,
        version: Option<u32>,
    ) -> Fut<'_, ArtifactVersion> {
        let (me, caller, artifact) = (self.0.clone(), caller.clone(), artifact.clone());
        Box::pin(async move {
            let slots: Vec<Arc<Slot>> = lock(&me.state)
                .sessions
                .values()
                .filter(|s| caller.scope.sees(&s.app))
                .cloned()
                .collect();
            let meta = slots
                .iter()
                .flat_map(|s| artifacts(s))
                .filter(|a| a.artifact == artifact && version.is_none_or(|v| v == a.version))
                .max_by_key(|a| a.version)
                .ok_or(HostError::NotFound)?;
            let blob = me.deps.blobs.get(&meta.blob).await?;
            if !caller.scope.sees(&blob.owner) {
                return Err(HostError::NotFound);
            }
            Ok(ArtifactVersion {
                meta,
                bytes: blob.bytes,
            })
        })
    }

    /// Every artifact version a session wrote.
    pub fn list_artifacts(
        &self,
        caller: &Caller,
        session: &SessionId,
    ) -> Fut<'_, Vec<ArtifactEvent>> {
        let (me, caller, session) = (self.0.clone(), caller.clone(), session.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &session).await?;
            Ok(artifacts(&slot))
        })
    }

    /// The audit record of every tool call and approval (shell only).
    pub fn activity(&self, caller: &Caller, app: Option<AppId>) -> Fut<'_, Vec<AuditRecord>> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move {
            if caller.scope != Scope::Shell {
                return Err(HostError::Unauthorized);
            }
            let st = lock(&me.state);
            Ok(st
                .audit
                .iter()
                .filter(|r| app.as_ref().is_none_or(|a| a == &r.app))
                .cloned()
                .collect())
        })
    }

    /// Engine start: load every session, sealing interrupted turns.
    pub fn recover(&self) -> Fut<'_, u64> {
        let me = self.0.clone();
        Box::pin(async move {
            let metas = me.deps.store.list_meta(None).await?;
            let shell = Caller::shell();
            for m in &metas {
                me.load(&shell, &m.id).await?;
            }
            Ok(metas.len() as u64)
        })
    }

    /// The broker saw an app connect.
    pub fn client_connected(&self, app: &AppId) -> BoxFuture<'_, ()> {
        self.client_change(app.clone(), LoopInput::ClientBack)
    }

    /// The broker saw an app disconnect.
    pub fn client_disconnected(&self, app: &AppId) -> BoxFuture<'_, ()> {
        self.client_change(app.clone(), LoopInput::ClientGone)
    }

    fn client_change(&self, app: AppId, input: LoopInput) -> BoxFuture<'_, ()> {
        let me = self.0.clone();
        Box::pin(async move {
            let slots: Vec<Arc<Slot>> = lock(&me.state)
                .sessions
                .values()
                .filter(|s| s.app == app)
                .cloned()
                .collect();
            for s in slots {
                me.apply(&s, input.clone()).await;
            }
        })
    }

    /// Engine stop: cancel in-flight turns and flush.
    pub fn shutdown(&self) -> BoxFuture<'_, ()> {
        let me = self.0.clone();
        Box::pin(async move {
            let slots: Vec<Arc<Slot>> = lock(&me.state).sessions.values().cloned().collect();
            for s in slots {
                if lock(&s.lp).running() {
                    me.apply(&s, LoopInput::Cancel).await;
                }
                me.flush(&s).await;
            }
        })
    }
}

fn artifacts(slot: &Slot) -> Vec<ArtifactEvent> {
    lock(&slot.lp)
        .events()
        .iter()
        .filter(|e| e.session == slot.id)
        .filter_map(|e| match &e.event {
            Event::Artifact(a) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

/// The `id` of TOML that did not validate, so it can still be listed.
fn toml_id(text: &str) -> Option<PresetId> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "id").then(|| PresetId::from(v.trim().trim_matches('"')))
    })
}
