//! Language-owned rendering dispatch, anchored to checked library source bytes.
use crate::{
    Loaded,
    views::{is, qualified},
};
use intersect_view_host::{ModelViewContext, ProjectObject};
use intersect_views::{RenderingOrigin, RenderingSelection};
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tree,
    Interconnection,
    GeneralDefault,
    InterconnectionDefault,
    ActionProfile,
    StateProfile,
    ActionStart,
    ActionDone,
}
struct Canonical {
    identity: String,
    uri: String,
    mounted: Option<(String, String)>,
    kind: Kind,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Intent {
    pub(super) registration_id: &'static str,
    pub(super) origin: RenderingOrigin,
    pub(super) origin_binding: intersect_views::Binding,
    pub(super) canonical_rendering: Option<intersect_views::Binding>,
}
pub(super) struct Registry {
    declarations: Vec<Canonical>,
}
impl Registry {
    pub(super) fn prepare(loaded: &Loaded) -> Result<Self, String> {
        let mut declarations = Vec::new();
        if loaded.library.is_empty() && loaded.texts.is_empty() {
            return Ok(Self { declarations });
        }
        #[cfg(feature = "components")]
        let descriptor = loaded
            .standard_component_payload
            .as_deref()
            .map(|payload| {
                let key = loaded.library_info["snapshotKey"]
                    .as_str()
                    .ok_or("Missing standard-library key")?;
                intersect_kerml::language_pack::standard_component_descriptor(payload, key)
            })
            .transpose()?;
        for (hash, entries) in [
            (
                "068b8cbad08f21f194a462d6923a7705a84bbf3ad31727f7f032834530633539",
                vec![
                    ("Views::asTreeDiagram", Kind::Tree, "RenderingUsage"),
                    (
                        "Views::asInterconnectionDiagram",
                        Kind::Interconnection,
                        "RenderingUsage",
                    ),
                ],
            ),
            (
                "2075fe906ddeefb5e8b38ee681acd3b6409a5ec2c1b04135a4a9d013c9d35ec8",
                vec![
                    (
                        "StandardViewDefinitions::GeneralView",
                        Kind::GeneralDefault,
                        "ViewDefinition",
                    ),
                    (
                        "StandardViewDefinitions::InterconnectionView",
                        Kind::InterconnectionDefault,
                        "ViewDefinition",
                    ),
                    (
                        "StandardViewDefinitions::ActionFlowView",
                        Kind::ActionProfile,
                        "ViewDefinition",
                    ),
                    (
                        "StandardViewDefinitions::StateTransitionView",
                        Kind::StateProfile,
                        "ViewDefinition",
                    ),
                ],
            ),
            (
                "f80cc391abd8f5d8171d581788c8595cd44811779a47259fd5d24680e3fe300b",
                vec![
                    ("Actions::Action::start", Kind::ActionStart, "ActionUsage"),
                    ("Actions::Action::done", Kind::ActionDone, "ActionUsage"),
                ],
            ),
        ] {
            let sources: Vec<_> = loaded
                .texts
                .iter()
                .filter(|(_, text)| format!("{:x}", Sha256::digest(text.as_bytes())) == hash)
                .collect();
            if sources.len() != 1 {
                return Err(
                    "Canonical rendering library source identity is absent or ambiguous".into(),
                );
            }
            let (uri, _) = sources[0];
            for (path, kind, class) in entries {
                let objects: Vec<_> = loaded
                    .library
                    .objects()
                    .filter(|o| {
                        o.resource() == uri
                            && is(&loaded.library, *o, class)
                            && qualified(*o) == path
                    })
                    .collect();
                if objects.len() != 1 {
                    return Err(format!(
                        "Missing/ambiguous canonical declaration in verified library: {path}"
                    ));
                }
                let identity = objects[0].identity();
                #[cfg(feature = "components")]
                let mounted = descriptor.as_ref().map(|descriptor| {
                    (
                        descriptor.mounted_object_id(identity),
                        descriptor.mounted_resource_uri(uri),
                    )
                });
                #[cfg(not(feature = "components"))]
                let mounted = None;
                declarations.push(Canonical {
                    identity: identity.into(),
                    mounted,
                    uri: uri.clone(),
                    kind,
                });
            }
        }
        Ok(Self { declarations })
    }
    // Capture checked library object identities in the language-owned module.
    // A user declaration with the same spelling never acquires control semantics.
    pub(super) fn control_functions(&self) -> Result<String, String> {
        if self.declarations.is_empty() {
            return Ok(concat!(
                "\nfn canonicalActionStart(e:k.Element)->Bool = false;\n",
                "\nfn canonicalActionDone(e:k.Element)->Bool = false;\n",
            )
            .to_owned());
        }
        let mut source = String::new();
        for (kind, function) in [
            (Kind::ActionStart, "canonicalActionStart"),
            (Kind::ActionDone, "canonicalActionDone"),
        ] {
            let declaration = self
                .declarations
                .iter()
                .find(|d| d.kind == kind)
                .ok_or("Missing verified action control declaration")?;
            let mut predicate = String::new();
            for identity in std::iter::once(&declaration.identity)
                .chain(declaration.mounted.as_ref().map(|(id, _)| id))
            {
                let identity = serde_json::to_string(identity).map_err(|e| e.to_string())?;
                if !predicate.is_empty() {
                    predicate.push_str(" || ");
                }
                predicate.push_str(&format!("identity(e) == {identity}"));
            }
            // FrozenStore enforces globally unique nonempty identities. Capture
            // the identity only after the source hash, URI and linked class have
            // all been verified above. `resource` is a direct-executor intrinsic
            // and must not be used by a stored-model view function.
            source.push_str(&format!(
                "\nfn {function}(e:k.Element)->Bool = {predicate};\n"
            ));
        }
        Ok(source)
    }
    fn canonical(&self, object: ProjectObject<'_>) -> Option<&Canonical> {
        self.declarations.iter().find(|c| {
            (c.identity == object.identity() && c.uri == object.resource())
                || c.mounted
                    .as_ref()
                    .is_some_and(|(id, uri)| id == object.identity() && uri == object.resource())
        })
    }
    // Interpret only verified canonical library declarations and their actual
    // linked ancestry. Intent is separate from availability of a presentation
    // registration: an unsupported package cannot change the language policy.
    pub(super) fn intent(
        &self,
        c: &ModelViewContext<'_>,
        view: ProjectObject<'_>,
    ) -> Result<Intent, String> {
        let mut defaults = Vec::new();
        for target in c.elements("viewDefinitionTypes", view)? {
            if let Some(known) = self.canonical(target)
                && matches!(
                    known.kind,
                    Kind::GeneralDefault
                        | Kind::InterconnectionDefault
                        | Kind::ActionProfile
                        | Kind::StateProfile
                )
            {
                defaults.push((known.kind, target));
            }
        }
        let behaviors: Vec<_> = defaults
            .iter()
            .copied()
            .filter(|(kind, _)| matches!(kind, Kind::ActionProfile | Kind::StateProfile))
            .collect();
        if behaviors.len() > 1 {
            return Err("Incomparable verified action/state view specializations".into());
        }
        // Remove only a verified ancestor, not an unrelated competing default.
        for (_, behavior) in &behaviors {
            let ancestors = c.elements("closure", *behavior)?;
            defaults.retain(|(kind, candidate)| {
                *kind != Kind::InterconnectionDefault
                    || !ancestors.iter().any(|a| {
                        a.identity() == candidate.identity() && a.resource() == candidate.resource()
                    })
            });
        }
        let members = c.elements("renderingMembers", view)?;
        if members.len() > 1 {
            return Err(
                "Competing effective rendering memberships; lawful redefinition is required".into(),
            );
        }
        let (kind, origin, origin_object, canonical) = if let Some(member) = members.first() {
            let mut targets = Vec::new();
            for target in c.elements("renderingTargets", *member)? {
                if let Some(known) = self.canonical(target)
                    && matches!(known.kind, Kind::Tree | Kind::Interconnection)
                    && !targets.iter().any(|(previous, _)| *previous == known.kind)
                {
                    targets.push((known.kind, target));
                }
            }
            if targets.len() != 1 {
                return Err(
                    "Explicit rendering has no single compatible verified registration".into(),
                );
            }
            let kind = match (targets[0].0, behaviors.first()) {
                (Kind::Tree, Some(_)) => {
                    return Err(
                        "Action/state profiles are incompatible with a parts tree rendering".into(),
                    );
                }
                (Kind::Interconnection, Some((profile, _))) => *profile,
                (kind, _) => kind,
            };
            let origins = c.elements("renderingOrigin", *member)?;
            let [origin_object] = origins.as_slice() else {
                return Err("Rendering membership has no unique source owner".into());
            };
            (
                kind,
                if origin_object.identity() == view.identity() {
                    RenderingOrigin::Explicit
                } else {
                    RenderingOrigin::Inherited
                },
                *origin_object,
                Some(targets[0].1),
            )
        } else {
            if defaults.len() != 1 {
                return Err(
                    "Absent rendering has no single verified view-definition default".into(),
                );
            }
            (defaults[0].0, RenderingOrigin::Default, defaults[0].1, None)
        };
        let registration_id = match kind {
            Kind::Tree => "sysml.partsTree",
            Kind::Interconnection | Kind::InterconnectionDefault => "sysml.interconnection",
            Kind::GeneralDefault => "sysml.compartments",
            Kind::ActionProfile => "sysml.actionFlow",
            Kind::StateProfile => "sysml.stateTransition",
            Kind::ActionStart | Kind::ActionDone => return Err("Control is not a rendering".into()),
        };
        Ok(Intent {
            registration_id,
            origin,
            origin_binding: c.binding(origin_object)?,
            canonical_rendering: canonical.map(|o| c.binding(o)).transpose()?,
        })
    }
    pub(super) fn resolve(
        &self,
        c: &ModelViewContext<'_>,
        view: ProjectObject<'_>,
    ) -> Result<RenderingSelection, String> {
        let intent = self.intent(c, view)?;
        let registry = c
            .rendering_registry()
            .ok_or("Rendering registry capability is unavailable")?;
        let registration = registry
            .registrations
            .iter()
            .find(|r| r.id == intent.registration_id)
            .ok_or("Required rendering registration is unavailable")?;
        let expected = if intent.registration_id == "sysml.partsTree" {
            intersect_views::ProjectionPolicy::ContainmentTree {
                edge_role: "part-child".into(),
                edge_notation: "containment".into(),
            }
        } else {
            intersect_views::ProjectionPolicy::Nested
        };
        if registration.rule_set_id != intent.registration_id
            || registration.projection_policy != expected
        {
            return Err(
                "Rendering registration is incompatible with language projection policy".into(),
            );
        }
        Ok(RenderingSelection {
            registration_id: intent.registration_id.into(),
            rule_set_id: registration.rule_set_id.clone(),
            package_fingerprint: c.presentation_fingerprint(),
            projection_policy: expected,
            origin: intent.origin,
            origin_binding: intent.origin_binding,
            canonical_rendering: intent.canonical_rendering,
        })
    }
}
