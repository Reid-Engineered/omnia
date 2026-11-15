use gpui::{
    App, Bounds, Context, FontWeight, SharedString, Window, WindowBounds, WindowHandle,
    WindowOptions, div, point, prelude::*, px, rgb, size,
};
use omnia_app::{STAGE_INK, STAGE_PAPER, Workspace, load_fonts, load_fonts_with};
use std::{
    borrow::Cow,
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

struct FontProbe {
    family: SharedString,
    weight: FontWeight,
    size_px: f32,
    text: &'static str,
}

impl Render for FontProbe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(STAGE_PAPER))
            .text_color(rgb(STAGE_INK))
            .p_4()
            .font_family(self.family.clone())
            .font_weight(self.weight)
            .text_size(px(self.size_px))
            .child(self.text)
    }
}

fn read_u16_be(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_u32_be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    assert_eq!(bytes.len() % 2, 0, "UTF-16BE string must have even length");
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&chunk| u16::from_be_bytes(chunk))
        .collect();
    String::from_utf16(&units).expect("valid UTF-16BE name record")
}

fn encode_utf16_be(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

fn sfnt_checksum(padded_bytes: &[u8]) -> u32 {
    assert_eq!(padded_bytes.len() % 4, 0);
    padded_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0u32, |acc, &chunk| {
            acc.wrapping_add(u32::from_be_bytes(chunk))
        })
}

#[derive(Clone, Copy)]
enum BundledFamily {
    Inter,
    SourceCodePro,
}

/// Rewrites the SFNT `name` table family records in a bundled TTF so the test registers
/// a distinct family name that cannot match any system-installed font.
fn patch_ttf_family(ttf: &[u8], inter_alias: &str, scp_alias: &str) -> Vec<u8> {
    assert!(ttf.len() >= 12, "invalid SFNT header");
    let num_tables = read_u16_be(ttf, 4) as usize;
    let dir_end = 12 + num_tables * 16;
    assert!(ttf.len() >= dir_end, "truncated SFNT table directory");

    struct TableEntry {
        tag: [u8; 4],
        data: Vec<u8>,
    }

    let mut tables = Vec::with_capacity(num_tables);
    let mut name_index = None;
    let mut head_index = None;
    for i in 0..num_tables {
        let entry = 12 + i * 16;
        let tag: [u8; 4] = ttf[entry..entry + 4].try_into().unwrap();
        let offset = read_u32_be(ttf, entry + 8) as usize;
        let length = read_u32_be(ttf, entry + 12) as usize;
        let data = ttf[offset..offset + length].to_vec();
        if &tag == b"name" {
            name_index = Some(i);
        } else if &tag == b"head" {
            head_index = Some(i);
        }
        tables.push(TableEntry { tag, data });
    }

    let name_idx = name_index.expect("TTF missing name table");
    let head_idx = head_index.expect("TTF missing head table");
    let name_bytes = &tables[name_idx].data;
    assert!(name_bytes.len() >= 6, "truncated name table header");
    let format = read_u16_be(name_bytes, 0);
    assert_eq!(format, 0, "expected format-0 name table");
    let count = read_u16_be(name_bytes, 2) as usize;
    let string_offset = read_u16_be(name_bytes, 4) as usize;
    assert!(
        name_bytes.len() >= 6 + count * 12,
        "truncated name record array"
    );

    struct NameRecord {
        platform_id: u16,
        encoding_id: u16,
        language_id: u16,
        name_id: u16,
        raw: Vec<u8>,
    }

    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let rec = 6 + i * 12;
        let platform_id = read_u16_be(name_bytes, rec);
        let encoding_id = read_u16_be(name_bytes, rec + 2);
        let language_id = read_u16_be(name_bytes, rec + 4);
        let name_id = read_u16_be(name_bytes, rec + 6);
        let len = read_u16_be(name_bytes, rec + 8) as usize;
        let off = read_u16_be(name_bytes, rec + 10) as usize;
        let start = string_offset + off;
        let raw = name_bytes[start..start + len].to_vec();
        records.push(NameRecord {
            platform_id,
            encoding_id,
            language_id,
            name_id,
            raw,
        });
    }

    let family = records
        .iter()
        .filter(|r| matches!(r.platform_id, 0 | 3) && matches!(r.name_id, 1 | 16 | 21))
        .find_map(|r| {
            let s = decode_utf16_be(&r.raw);
            if s == "Inter" || s.starts_with("Inter ") {
                Some(BundledFamily::Inter)
            } else if s == "Source Code Pro" || s.starts_with("Source Code Pro ") {
                Some(BundledFamily::SourceCodePro)
            } else {
                None
            }
        })
        .expect("unrecognized bundled font family in name table");

    for r in &mut records {
        if !matches!(r.platform_id, 0 | 3) || !matches!(r.name_id, 1 | 3 | 4 | 6 | 16 | 21) {
            continue;
        }
        let s = decode_utf16_be(&r.raw);
        let rewritten = match family {
            BundledFamily::Inter => s.replace("Inter", inter_alias),
            BundledFamily::SourceCodePro => s
                .replace("Source Code Pro", scp_alias)
                .replace("SourceCodePro", scp_alias),
        };
        r.raw = encode_utf16_be(&rewritten);
    }

    let new_string_offset = 6 + records.len() * 12;
    let mut new_name = Vec::new();
    new_name.extend_from_slice(&0u16.to_be_bytes());
    new_name.extend_from_slice(&(records.len() as u16).to_be_bytes());
    new_name.extend_from_slice(&(new_string_offset as u16).to_be_bytes());
    let mut string_storage = Vec::new();
    for r in &records {
        let offset = string_storage.len();
        string_storage.extend_from_slice(&r.raw);
        new_name.extend_from_slice(&r.platform_id.to_be_bytes());
        new_name.extend_from_slice(&r.encoding_id.to_be_bytes());
        new_name.extend_from_slice(&r.language_id.to_be_bytes());
        new_name.extend_from_slice(&r.name_id.to_be_bytes());
        new_name.extend_from_slice(&(r.raw.len() as u16).to_be_bytes());
        new_name.extend_from_slice(&(offset as u16).to_be_bytes());
    }
    new_name.extend_from_slice(&string_storage);
    tables[name_idx].data = new_name;

    assert!(tables[head_idx].data.len() >= 12, "truncated head table");
    tables[head_idx].data[8..12].copy_from_slice(&[0u8; 4]);

    let mut out = Vec::with_capacity(ttf.len() + 256);
    out.extend_from_slice(&ttf[..12]);
    out.resize(12 + tables.len() * 16, 0);

    let mut head_offset_in_file = 0usize;
    for (i, table) in tables.iter().enumerate() {
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let offset = out.len();
        if i == head_idx {
            head_offset_in_file = offset;
        }
        let length = table.data.len();
        out.extend_from_slice(&table.data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let checksum = sfnt_checksum(&out[offset..]);
        let dir_pos = 12 + i * 16;
        out[dir_pos..dir_pos + 4].copy_from_slice(&table.tag);
        out[dir_pos + 4..dir_pos + 8].copy_from_slice(&checksum.to_be_bytes());
        out[dir_pos + 8..dir_pos + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[dir_pos + 12..dir_pos + 16].copy_from_slice(&(length as u32).to_be_bytes());
    }

    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&out));
    out[head_offset_in_file + 8..head_offset_in_file + 12]
        .copy_from_slice(&adjustment.to_be_bytes());
    out
}

fn count_non_paper_pixels(raw: &[u8], width: u32, height: u32, paper_rgba: [u8; 4]) -> usize {
    let mut count = 0;
    for y in 0..height {
        let row_start = (y * width * 4) as usize;
        let row_end = ((y + 1) * width * 4) as usize;
        for pixel in raw[row_start..row_end].as_chunks::<4>().0 {
            if *pixel != paper_rgba {
                count += 1;
            }
        }
    }
    count
}

fn count_ink_row_bands(raw: &[u8], width: u32, height: u32, paper_rgba: [u8; 4]) -> usize {
    let mut bands = 0;
    let mut in_band = false;
    for y in 0..height {
        let row_start = (y * width * 4) as usize;
        let row_end = ((y + 1) * width * 4) as usize;
        let row_has_ink = raw[row_start..row_end]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| *pixel != paper_rgba);
        if row_has_ink && !in_band {
            bands += 1;
            in_band = true;
        } else if !row_has_ink {
            in_band = false;
        }
    }
    bands
}

fn main() {
    // Rule 9: ensure even a stalled hidden-window test exits cleanly in CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("fonts test did not finish within 30s");
        std::process::exit(3);
    });

    let pid = std::process::id();
    let inter_alias: SharedString = format!("OmniaBundledInter{pid}").into();
    let scp_alias: SharedString = format!("OmniaBundledSCP{pid}").into();
    let missing_family: SharedString = format!("OmniaMissingFont{pid}").into();

    let outcome = Arc::new(Mutex::new(None::<Result<(), String>>));
    let outcome_for_run = outcome.clone();

    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(err) = load_fonts(cx) {
            *outcome_for_run.lock().unwrap() =
                Some(Err(format!("register bundled Omnia fonts: {err}")));
            cx.quit();
            return;
        }

        let inter_alias_for_patch = inter_alias.clone();
        let scp_alias_for_patch = scp_alias.clone();
        if let Err(err) = load_fonts_with(cx, move |bytes| {
            Cow::Owned(patch_ttf_family(
                bytes,
                &inter_alias_for_patch,
                &scp_alias_for_patch,
            ))
        }) {
            *outcome_for_run.lock().unwrap() =
                Some(Err(format!("register aliased bundled Omnia fonts: {err}")));
            cx.quit();
            return;
        }

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(160.0)));
        let workspace_win = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| Workspace::new(None))
                },
            )
            .expect("open hidden workspace window");

        let open_probe = |cx: &mut App, family: SharedString, weight: FontWeight, size_px: f32| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| FontProbe {
                        family,
                        weight,
                        size_px,
                        text: "Omnia 0123456789 R1 10 kOhm",
                    })
                },
            )
            .expect("open hidden probe window")
        };

        let inter_alias_win = open_probe(cx, inter_alias.clone(), FontWeight::SEMIBOLD, 17.0);
        let inter_real_win = open_probe(cx, "Inter".into(), FontWeight::SEMIBOLD, 17.0);
        let scp_alias_win = open_probe(cx, scp_alias.clone(), FontWeight::MEDIUM, 13.0);
        let scp_real_win = open_probe(cx, "Source Code Pro".into(), FontWeight::MEDIUM, 13.0);
        let fallback_inter_win =
            open_probe(cx, missing_family.clone(), FontWeight::SEMIBOLD, 17.0);
        let fallback_scp_win = open_probe(cx, missing_family.clone(), FontWeight::MEDIUM, 13.0);

        cx.spawn(async move |cx| {
            let res = cx.update(|cx| -> Result<(), String> {
                let font_names = cx.text_system().all_font_names();
                for expected in [
                    inter_alias.as_ref(),
                    scp_alias.as_ref(),
                    "Inter",
                    "Source Code Pro",
                ] {
                    if !font_names.iter().any(|n| n == expected) {
                        return Err(format!(
                            "expected {expected:?} in registered fonts ({} total)",
                            font_names.len()
                        ));
                    }
                }

                let mut inter_ids = HashSet::new();
                let mut scp_ids = HashSet::new();
                for weight in [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD] {
                    let mut inter_font = gpui::font(inter_alias.clone());
                    inter_font.weight = weight;
                    let inter_id = cx.text_system().resolve_font(&inter_font);

                    let mut scp_font = gpui::font(scp_alias.clone());
                    scp_font.weight = weight;
                    let scp_id = cx.text_system().resolve_font(&scp_font);

                    let mut fallback_font = gpui::font(missing_family.clone());
                    fallback_font.weight = weight;
                    let fallback_id = cx.text_system().resolve_font(&fallback_font);

                    if inter_id == fallback_id {
                        return Err(format!(
                            "aliased Inter at weight {weight:?} fell back to system fallback"
                        ));
                    }
                    if scp_id == fallback_id {
                        return Err(format!(
                            "aliased Source Code Pro at weight {weight:?} fell back to system fallback"
                        ));
                    }
                    if inter_id == scp_id {
                        return Err(format!(
                            "expected aliased Inter and Source Code Pro at weight {weight:?} to resolve to distinct FontIds"
                        ));
                    }
                    if !inter_ids.insert(inter_id) {
                        return Err(format!(
                            "expected aliased Inter weight {weight:?} to resolve to its own bundled FontId"
                        ));
                    }
                    if !scp_ids.insert(scp_id) {
                        return Err(format!(
                            "expected aliased Source Code Pro weight {weight:?} to resolve to its own bundled FontId"
                        ));
                    }
                }

                let paper_rgba = [
                    ((STAGE_PAPER >> 16) & 0xFF) as u8,
                    ((STAGE_PAPER >> 8) & 0xFF) as u8,
                    (STAGE_PAPER & 0xFF) as u8,
                    0xFF,
                ];

                let workspace_img = cx
                    .update_window(workspace_win.into(), |_, window, cx| {
                        window.draw(cx).clear(cx);
                        window
                            .render_to_image()
                            .expect("render workspace window to image")
                    })
                    .map_err(|e| format!("update workspace window: {e}"))?;

                let w = workspace_img.width();
                let h = workspace_img.height();
                if w < 640 || h < 160 {
                    return Err(format!(
                        "expected resized hidden workspace window >= 640x160, got {w}x{h}"
                    ));
                }

                let total_ink = count_non_paper_pixels(workspace_img.as_raw(), w, h, paper_rgba);
                let bands = count_ink_row_bands(workspace_img.as_raw(), w, h, paper_rgba);
                if total_ink == 0 || bands < 2 {
                    return Err(format!(
                        "expected two rendered text lines in Workspace (w={w}, h={h}, total_ink={total_ink}, bands={bands})"
                    ));
                }

                let render_probe_img =
                    |cx: &mut App, win: WindowHandle<FontProbe>| -> Result<_, String> {
                        cx.update_window(win.into(), |_, window, cx| {
                            window.draw(cx).clear(cx);
                            window
                                .render_to_image()
                                .expect("render font probe to image")
                        })
                        .map_err(|e| format!("update probe window: {e}"))
                    };

                let inter_alias_img = render_probe_img(cx, inter_alias_win)?;
                let inter_real_img = render_probe_img(cx, inter_real_win)?;
                let scp_alias_img = render_probe_img(cx, scp_alias_win)?;
                let scp_real_img = render_probe_img(cx, scp_real_win)?;
                let fallback_inter_img = render_probe_img(cx, fallback_inter_win)?;
                let fallback_scp_img = render_probe_img(cx, fallback_scp_win)?;

                let inter_ink = count_non_paper_pixels(
                    inter_alias_img.as_raw(),
                    inter_alias_img.width(),
                    inter_alias_img.height(),
                    paper_rgba,
                );
                let scp_ink = count_non_paper_pixels(
                    scp_alias_img.as_raw(),
                    scp_alias_img.width(),
                    scp_alias_img.height(),
                    paper_rgba,
                );
                if inter_ink == 0 {
                    return Err("expected aliased Inter render to draw non-background pixels".into());
                }
                if scp_ink == 0 {
                    return Err(
                        "expected aliased Source Code Pro render to draw non-background pixels"
                            .into(),
                    );
                }
                if inter_alias_img.as_raw() == fallback_inter_img.as_raw() {
                    return Err("aliased Inter render matched fallback font render".into());
                }
                if scp_alias_img.as_raw() == fallback_scp_img.as_raw() {
                    return Err(
                        "aliased Source Code Pro render matched fallback font render".into(),
                    );
                }
                if inter_alias_img.as_raw() != inter_real_img.as_raw() {
                    return Err("Inter render did not match aliased bundled Inter bytes".into());
                }
                if scp_alias_img.as_raw() != scp_real_img.as_raw() {
                    return Err(
                        "Source Code Pro render did not match aliased bundled Source Code Pro bytes"
                            .into(),
                    );
                }
                if inter_alias_img.as_raw() == scp_alias_img.as_raw() {
                    return Err(
                        "expected aliased Inter and Source Code Pro to produce different renders"
                            .into(),
                    );
                }

                Ok(())
            });

            *outcome_for_run.lock().unwrap() = Some(res);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let res = outcome
        .lock()
        .unwrap()
        .take()
        .expect("test async task did not run before application exited");
    if let Err(err) = res {
        panic!("{err}");
    }
    println!("test bundled_fonts_register_and_render_distinctly ... ok");
}
