//! The generated themes: 22 colour tokens from omnia.figcraft plus the 17 overlay tokens
//! (13 from Q2 and 4 from UQ6).

use omnia_theme::{GRAPHITE, GRAY, Rgba, Theme};

#[test]
fn both_themes_have_every_token() {
    assert_eq!(Theme::TOKEN_COUNT, 39);
    for theme in [&GRAPHITE, &GRAY] {
        let tokens = theme.tokens();
        assert!(tokens.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (name, colour) in tokens {
            let channels = [colour.r, colour.g, colour.b, colour.a];
            assert!(
                channels.iter().all(|c| (0.0..=1.0).contains(c)),
                "{name} = {colour:?}"
            );
        }
    }
}

#[test]
fn overlay_values_follow_q2() {
    let hex = |rgb: u32, a: u8| {
        let byte = |shift: u32| f32::from(((rgb >> shift) & 0xff) as u8) / 255.0;
        Rgba::new(byte(16), byte(8), byte(0), f32::from(a) / 255.0)
    };
    for theme in [&GRAPHITE, &GRAY] {
        assert_eq!(theme.stage_ink, hex(0x1D1D1F, 0xFF));
        assert_eq!(theme.stage_grid, hex(0x000000, 0x22));
        assert_eq!(theme.stage_select, hex(0x2F6FEB, 0xFF));
        assert_eq!(theme.probe_b, hex(0x7A5AF8, 0xFF));
        assert_eq!(theme.stage_hover, theme.stage_select_halo);
        assert_eq!(theme.stage_net_highlight, theme.stage_select);
        assert_eq!(theme.probe_a, theme.stage_select);
        assert_eq!(theme.stage_snap, theme.accent_mint);
    }
    assert_ne!(GRAPHITE.stage_snap, GRAY.stage_snap);
    assert_eq!(GRAPHITE.stage_paper, GRAY.stage_paper);
}

#[test]
fn overlay_values_follow_uq6() {
    let hex = |rgb: u32, a: u8| {
        let byte = |shift: u32| f32::from(((rgb >> shift) & 0xff) as u8) / 255.0;
        Rgba::new(byte(16), byte(8), byte(0), f32::from(a) / 255.0)
    };
    for theme in [&GRAPHITE, &GRAY] {
        assert_eq!(theme.brand_tile, hex(0xF2F2F2, 0xFF));
        assert_eq!(theme.brand_mark, hex(0x111112, 0xFF));
        assert_eq!(theme.instrument_screen, hex(0x0E0F11, 0xFF));
        assert_eq!(theme.focus_ring, theme.stage_select);
    }
}
