//! Host-built, digest-checked standard-library component cache. No acquisition
//! or runtime preparation fallback: the native pack builder issues the proof.
use super::*;
use intersect_model::{
    components::{ComponentDescriptor, ComponentImage, SharedSchema},
    frozen::{ById, FrozenStore, Model, SnapshotTrust},
    storage::{self, ModelStore},
};

pub const STANDARD_COMPONENT_URI: &str = "urn:intersect:standard-library";
const KIND: &str = "intersect.kerml.standard-component.v1";
const MAGIC: &[u8; 8] = b"IXSCMP\0\0";
const MAX_HEADER: usize = 65536;
const MAX_SNAPSHOT: usize = 256 << 20;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    library_key: String,
    library_payload_digest: String,
    descriptor: ComponentDescriptor,
}

fn key(library_key: &str, library_payload_digest: &str) -> Result<String, String> {
    intersect_model::definitions::library_part::key_bytes(library_key)
        .map_err(|e| e.to_string())?;
    intersect_model::definitions::library_part::key_bytes(library_payload_digest)
        .map_err(|e| e.to_string())?;
    Ok(FileDigest::of(
        &canonical_json(&(
            KIND,
            library_key,
            library_payload_digest,
            expected_compiler(),
        ))
        .map_err(|e| e.to_string())?,
    )
    .sha256)
}

fn entry(
    library_key: &str,
    library_payload_digest: &str,
    output_digest: String,
) -> Result<PackPart, String> {
    let mut components: Vec<_> = PROGRAM_COMPONENTS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    components.sort();
    Ok(PackPart {
        name: STANDARD_COMPONENT_SECTION.into(),
        components,
        inputs: vec![],
        depends: vec![LIBRARY_SECTION.into()],
        input_digest: key(library_key, library_payload_digest)?,
        output_digest,
    })
}

fn decode<'p>(payload: &'p [u8], library_key: &str) -> Result<(Header, &'p [u8]), String> {
    if payload.get(..8) != Some(MAGIC.as_slice()) {
        return Err("Invalid standard component cache magic".into());
    }
    let length = payload
        .get(8..12)
        .ok_or("Truncated standard component header")?;
    let length = u32::from_le_bytes(
        length
            .try_into()
            .map_err(|_| "Invalid component header length")?,
    ) as usize;
    if length > MAX_HEADER {
        return Err("Standard component header exceeds its bound".into());
    }
    let end = 12usize
        .checked_add(length)
        .ok_or("Component header length overflow")?;
    let header: Header = serde_json::from_slice(
        payload
            .get(12..end)
            .ok_or("Truncated standard component header")?,
    )
    .map_err(|e| format!("Standard component header: {e}"))?;
    if header.version != 1 || header.library_key != library_key {
        return Err("Standard component cache belongs to a different library or format".into());
    }
    let snapshot = payload
        .get(end..)
        .ok_or("Missing standard component snapshot")?;
    if snapshot.is_empty() || snapshot.len() > MAX_SNAPSHOT {
        return Err("Standard component snapshot exceeds its bound".into());
    }
    Ok((header, snapshot))
}

/// Native build-time preparation. Metadata-only repackaging preserves native
/// slots and assignment evidence; the ordinary component factory performs its
/// full canonical audit before any cache bytes are returned to the host.
pub fn prepare_standard_component_cache(
    context: Arc<SharedSchema>,
    library_key: &str,
    library_payload: &[u8],
) -> Result<Vec<u8>, String> {
    let library_payload_digest = FileDigest::of(library_payload).sha256;
    key(library_key, &library_payload_digest)?;
    if payload_key(library_payload)? != library_key {
        return Err("Component preparation requires the selected library snapshot".into());
    }
    let part = intersect_model::definitions::library_part::decode_library_part(library_payload)
        .map_err(|e| e.to_string())?;
    // Read the exact selected payload, rather than accepting a separately
    // supplied model that could accidentally belong to another library.
    let library = Model::from_snapshot(
        context.metamodel(),
        part.snapshot,
        &intersect_model::definitions::library_part::key_bytes(library_key)
            .map_err(|e| e.to_string())?,
        SnapshotTrust::Revalidate,
    )
    .map_err(|e| e.to_string())?;
    let records = library.resource_records().map_err(|e| e.to_string())?;
    let mut store = FrozenStore::from_model(library);
    storage::atomic(&mut store, |store| {
        for mut record in records.into_values() {
            record.package = None;
            store.set_resource_record(record)?;
        }
        Ok(())
    })?;
    let closed = store
        .into_builder()
        .map_err(|e| e.to_string())?
        .link(&ById)
        .map_err(|e| format!("{e:?}"))?
        .validate()
        .map_err(|e| format!("{e:?}"))?;
    let (image, snapshot) =
        ComponentImage::prepare_with_snapshot(context, STANDARD_COMPONENT_URI, closed)?;
    if snapshot.len() > MAX_SNAPSHOT {
        return Err("Standard component snapshot exceeds its bound".into());
    }
    let header = canonical_json(&Header {
        version: 1,
        library_key: library_key.into(),
        library_payload_digest,
        descriptor: image.descriptor(),
    })
    .map_err(|e| e.to_string())?;
    if header.len() > MAX_HEADER {
        return Err("Standard component header exceeds its bound".into());
    }
    let mut payload = Vec::with_capacity(12 + header.len() + snapshot.len());
    payload.extend_from_slice(MAGIC);
    payload.extend_from_slice(&(header.len() as u32).to_le_bytes());
    payload.extend_from_slice(&header);
    payload.extend_from_slice(&snapshot);
    Ok(payload)
}

/// Restore a prepared image using Model's explicit host-cache trust boundary.
/// `VerifiedHostCache` requires a verified enclosing pack section. Downloaded
/// KPARs must continue through their textual preparation and canonical audit.
pub fn load_standard_component_cache(
    context: Arc<SharedSchema>,
    payload: &[u8],
    library_key: &str,
    trust: SnapshotTrust,
) -> Result<Arc<ComponentImage>, String> {
    let (header, snapshot) = decode(payload, library_key)?;
    let image = ComponentImage::restore_with_trust(context, &header.descriptor, snapshot, trust)?;
    if image.uri() != STANDARD_COMPONENT_URI {
        return Err("Prepared standard component has another usage URI".into());
    }
    Ok(image)
}

/// Only a verified section with an exact recipe/library key is readable.
pub fn pack_standard_component_payload<'p>(
    pack: &'p LanguagePack,
    library_key: &str,
) -> Result<Option<&'p [u8]>, String> {
    check_layout(pack.manifest())?;
    let Some(part) = pack
        .manifest()
        .parts
        .iter()
        .find(|p| p.name == STANDARD_COMPONENT_SECTION)
    else {
        return Ok(None);
    };
    let library_digest = &pack
        .section_entry(LIBRARY_SECTION)
        .ok_or("Missing component dependency")?
        .digest
        .sha256;
    if *part != entry(library_key, library_digest, part.output_digest.clone())? {
        return Err("Standard component part belongs to another compiler or library".into());
    }
    let section = pack
        .section_entry(STANDARD_COMPONENT_SECTION)
        .ok_or("Missing standard component section")?;
    if section.kind != KIND
        || section.encoding != intersect_model::definitions::pack::PackSectionEncoding::Binary
        || section.flags != PackSectionFlags::REQUIRED
        || section.part_digest != part.input_digest
        || section.output_digest != part.output_digest
        || section.digest.sha256 != part.output_digest
    {
        return Err("Invalid standard component section metadata".into());
    }
    let payload = pack
        .required_section(STANDARD_COMPONENT_SECTION)
        .map_err(|e| e.to_string())?;
    let (header, _) = decode(payload, library_key)?;
    if header.library_payload_digest != *library_digest {
        return Err("Standard component cache belongs to different library bytes".into());
    }
    Ok(Some(payload))
}

pub fn with_standard_component_part(
    pack: &LanguagePack,
    payload: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let library = pack_library_payload(pack)
        .ok_or("A standard component cache requires its library part")??;
    let library_key = payload_key(library)?;
    let (header, _) = decode(&payload, &library_key)?;
    let library_digest = &pack
        .section_entry(LIBRARY_SECTION)
        .ok_or("Missing component dependency")?
        .digest
        .sha256;
    if header.library_payload_digest != *library_digest {
        return Err("Standard component cache belongs to different library bytes".into());
    }
    replace(
        pack,
        Some((
            entry(
                &library_key,
                library_digest,
                FileDigest::of(&payload).sha256,
            )?,
            payload,
        )),
    )
}

/// A classic-only pack remains usable for ordinary projects. Hosted usage with
/// a nonempty stdlib explicitly requires its prepared component cache.
pub fn without_standard_component_part(pack: &LanguagePack) -> Result<Vec<u8>, String> {
    replace(pack, None)
}

fn replace(pack: &LanguagePack, added: Option<(PackPart, Vec<u8>)>) -> Result<Vec<u8>, String> {
    check_layout(pack.manifest())?;
    let manifest = pack.manifest();
    let mut parts: Vec<_> = manifest
        .parts
        .iter()
        .filter(|p| p.name != STANDARD_COMPONENT_SECTION)
        .cloned()
        .collect();
    if let Some((part, _)) = &added {
        parts.push(part.clone());
    }
    let mut builder = PackBuilder::new(PackHeader {
        language: manifest.language.clone(),
        recipe: manifest.recipe.clone(),
        compiler: manifest.compiler.clone(),
        recipe_constants: manifest.recipe_constants.clone(),
        inputs: manifest.inputs.clone(),
        parts,
        requires_hooks: manifest.requires_hooks.clone(),
        depends_on: manifest.depends_on.clone(),
    });
    for section in &manifest.sections {
        if section.name == STANDARD_COMPONENT_SECTION {
            continue;
        }
        let bytes = pack
            .required_section(&section.name)
            .map_err(|e| e.to_string())?
            .to_vec();
        builder = match section.encoding {
            intersect_model::definitions::pack::PackSectionEncoding::Binary => builder
                .binary_section(
                    section.name.clone(),
                    section.kind.clone(),
                    section.flags,
                    bytes,
                    section.part_digest.clone(),
                    section.output_digest.clone(),
                ),
            _ => builder.section(
                section.name.clone(),
                section.kind.clone(),
                section.flags,
                bytes,
                section.part_digest.clone(),
                section.output_digest.clone(),
            ),
        };
    }
    if let Some((part, payload)) = added {
        builder = builder.binary_section(
            STANDARD_COMPONENT_SECTION,
            KIND,
            PackSectionFlags::REQUIRED,
            payload,
            part.input_digest,
            part.output_digest,
        );
    }
    builder.build().map_err(|e| e.to_string())
}
