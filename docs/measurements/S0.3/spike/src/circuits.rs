//! Built-in SPICE circuits for the SS1 (S0.3) benchmarks and verification tests.
//!
//! All numeric values are emitted in plain decimal/exponent form (`SINV6`) and all
//! macromodels use standard `R`/`C`/`E`/`G`/`D` primitives (`SD3`).

/// `STC-lin-01` style resistive divider + bridge (`10 V`, `1k`/`2k`/`3k`/`4k`).
/// Expected: `v(n1) = 10 * 2k/(1k+2k) = 6.6666666667 V`, `v(n2) = 10 * 4k/(3k+4k) = 5.7142857143 V`.
pub fn simple_op_netlist() -> Vec<String> {
    vec![
        "* SS1 simple OP divider bridge".into(),
        "V1 vin 0 DC 10".into(),
        "R1 vin n1 1000".into(),
        "R2 n1 0 2000".into(),
        "R3 vin n2 3000".into(),
        "R4 n2 0 4000".into(),
        ".options reltol=1e-3 abstol=1e-12 vntol=1e-6 itl1=100".into(),
        ".end".into(),
    ]
}

/// `STC-wien-01` style Wien bridge oscillator (`R1=R2=10k`, `C1=C2=22n`, `f0 ≈ 723.4 Hz`)
/// with a pure `R`/`C`/`E`/`G`/`D` behavioral op-amp macromodel and `±12 V` rails.
pub fn wien_tran_netlist() -> Vec<String> {
    vec![
        "* SS1 Wien bridge oscillator (behavioral macromodel, R/C/E/G/D only)".into(),
        ".subckt opamp_beh inp inn vp vn out".into(),
        "Rin inp inn 1e6".into(),
        "G1 0 mid inp inn 1e-3".into(),
        "Rpole mid 0 1e8".into(),
        "Cpole mid 0 5.30516477e-11".into(),
        "Ebuf buf 0 mid 0 1".into(),
        "Rout buf out 75".into(),
        "Dhi out vp dclamp".into(),
        "Dlo vn out dclamp".into(),
        ".model dclamp D(IS=1e-14 N=1)".into(),
        ".ends opamp_beh".into(),
        ".model dsig D(IS=2.52e-9 RS=0.568 N=1.752 BV=100 IBV=1e-4)".into(),
        "Vpos vcc 0 DC 12".into(),
        "Vneg vee 0 DC -12".into(),
        "X1 nplus nminus vcc vee vout opamp_beh".into(),
        "R1 vout nmid 10000".into(),
        "C1 nmid nplus 2.2e-8".into(),
        "R2 nplus 0 10000".into(),
        "C2 nplus 0 2.2e-8".into(),
        "R3a vout nminus 22000".into(),
        "R4 nminus 0 10000".into(),
        "D1 vout nminus dsig".into(),
        "D2 nminus vout dsig".into(),
        ".ic v(nplus)=0.05".into(),
        ".options reltol=1e-3 abstol=1e-12 vntol=1e-6 itl1=100".into(),
        ".tran 2e-5 0.04 0 2e-5 uic".into(),
        ".end".into(),
    ]
}

/// High-sample-count transient (`>= 100,000` accepted time points, 2 probe channels)
/// to measure sustained binary `f64` streaming throughput over worker IPC vs in-process FFI.
pub fn stream_tran_netlist() -> Vec<String> {
    vec![
        "* SS1 100k-point streaming throughput circuit".into(),
        "V1 vin 0 SIN(0 5 1000)".into(),
        "R1 vin vout 1000".into(),
        "C1 vout 0 1e-7".into(),
        ".options reltol=1e-3 abstol=1e-12 vntol=1e-6".into(),
        ".tran 1e-6 0.1 0 1e-6".into(),
        ".end".into(),
    ]
}

/// Heavy long-running nonlinear transient (many diode stages + tight max step) that runs for
/// several seconds so cancellation (`bg_halt` + kill-on-cancel) always hits mid-solve (`SB4`).
pub fn heavy_cancel_tran_netlist() -> Vec<String> {
    let mut lines = vec![
        "* SS1 heavy nonlinear transient for cancel/kill benchmark (SB4)".into(),
        ".model dsig D(IS=2.52e-9 RS=0.568 N=1.752)".into(),
        "V1 n0 0 SIN(0 5 10000)".into(),
    ];
    // 24-stage nonlinear diode-RC ladder: non-trivial Newton iteration per step.
    for i in 0..24 {
        let a = format!("n{i}");
        let b = format!("n{}", i + 1);
        lines.push(format!("R{i} {a} {b} 1000"));
        lines.push(format!("C{i} {b} 0 1e-9"));
        lines.push(format!("D{i}a {b} 0 dsig"));
        lines.push(format!("D{i}b 0 {b} dsig"));
    }
    lines.push(".options reltol=1e-4 abstol=1e-12 vntol=1e-6".into());
    lines.push(".tran 1e-7 0.2 0 1e-7".into());
    lines.push(".end".into());
    lines
}
