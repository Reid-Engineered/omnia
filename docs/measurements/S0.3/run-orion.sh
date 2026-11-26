#!/usr/bin/env bash
# S0.3 / SS1 one-command Orion Linux campaign script.
#
# Builds ngspice-47 as a shared library (libngspice.so) with every optional GPL/LGPL/MPL
# feature disabled, audits its dynamic dependencies (ldd + readelf), builds the standalone
# spike crate, and runs the 10-repetition rotating SS1 benchmark under canvas plan §2.1
# conditions into an atomically claimed run directory.
#
# Usage (from repo root on Orion, on AC, governors=performance, other apps closed):
#   bash docs/measurements/S0.3/run-orion.sh [label] [work_dir]
#
# Defaults:
#   label    = orion
#   work_dir = /tmp/omnia-s03-ngspice
set -euo pipefail
unset CDPATH

NGSPICE_VER="47"
NGSPICE_URL="https://downloads.sourceforge.net/project/ngspice/ng-spice-rework/${NGSPICE_VER}/ngspice-${NGSPICE_VER}.tar.gz"
NGSPICE_SHA256="894e649651f1838a14095e5a5439e7d3aa63e87ede14d283173fda4fcdef675f"

CONFIGURE_FLAGS=(
  --disable-dependency-tracking
  --with-ngshared
  --disable-debug
  --enable-xspice
  --disable-cider
  --disable-klu
  --disable-osdi
  --disable-openmp
  --without-x
  --with-readline=no
  --with-editline=no
  --with-fftw3=no
)
BUILD_SUBDIRS=(
  src/misc
  src/maths
  src/frontend
  src/spicelib
  src/xspice/mif
  src/xspice/cm
  src/xspice/enh
  src/xspice/evt
  src/xspice/idn
)
BUILD_CFLAGS="-O2"

# Deterministic stamp binding any reused source/build/install tree to the exact verified
# tarball hash, configure flags, CFLAGS, and built subdirs.
BUILD_STAMP="$(
  printf 'ver=%s\nsha256=%s\ncflags=%s\nflags=%s\nsubdirs=%s\n' \
    "$NGSPICE_VER" "$NGSPICE_SHA256" "$BUILD_CFLAGS" \
    "${CONFIGURE_FLAGS[*]}" "${BUILD_SUBDIRS[*]}" \
  | sha256sum | awk '{print $1}'
)"

# Fixed benchmark schedule (same as Windows diagnostic run).
REPS=10
STARTUP_ITERS=10
STREAM_ITERS=3
CANCEL_ITERS=5
BATCH_SIZE=256

label="${1:-orion}"
work_dir="${2:-/tmp/omnia-s03-ngspice}"
mkdir -p "$work_dir"
work_dir="$(CDPATH='' cd -- "$work_dir" && pwd)"
here="$(CDPATH='' cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Claim an atomic run directory; plain mkdir (no -p) fails if it already exists so no
# concurrent or repeated run can overwrite committed campaign data.
mkdir -p "$here/results/runs"
results="$here/results/runs/${label}-$(date +%Y%m%dT%H%M%S)"
mkdir "$results" || { echo "$results already exists; refusing to overwrite a run" >&2; exit 1; }

echo "==> S0.3 / SS1 campaign (label=${label})"
echo "    work_dir: ${work_dir}"
echo "    results:  ${results}"

tarball="$work_dir/ngspice-${NGSPICE_VER}.tar.gz"
if [ ! -f "$tarball" ]; then
  echo "==> Downloading ngspice-${NGSPICE_VER}..."
  curl -fL --retry 3 -o "$tarball" "$NGSPICE_URL"
fi

echo "==> Verifying tarball SHA-256..."
actual_sha="$(sha256sum "$tarball" | awk '{print $1}')"
if [ "$actual_sha" != "$NGSPICE_SHA256" ]; then
  echo "ERROR: tarball SHA-256 mismatch: got $actual_sha, expected $NGSPICE_SHA256" >&2
  exit 1
fi
printf '%s  ngspice-%s.tar.gz\n' "$actual_sha" "$NGSPICE_VER" > "$results/tarball-${label}.sha256"

src_dir="$work_dir/ngspice-${NGSPICE_VER}"
src_stamp="$src_dir/.omnia-s03-src.stamp"
if [ ! -f "$src_stamp" ] || [ "$(cat "$src_stamp")" != "$NGSPICE_SHA256" ]; then
  echo "==> Extracting verified ngspice-${NGSPICE_VER} source tree..."
  rm -rf "$src_dir"
  tar -xzf "$tarball" -C "$work_dir"
  printf '%s\n' "$NGSPICE_SHA256" > "$src_stamp"
fi

build_dir="$work_dir/build-linux"
install_dir="$work_dir/install-linux"
build_stamp_file="$install_dir/.omnia-s03-build.stamp"
lib_path="$install_dir/lib/libngspice.so"

if [ ! -f "$build_stamp_file" ] || [ "$(cat "$build_stamp_file")" != "$BUILD_STAMP" ] || [ ! -f "$lib_path" ]; then
  echo "==> Configuring and building libngspice.so (stamp=${BUILD_STAMP:0:12})..."
  rm -rf "$build_dir" "$install_dir"
  mkdir -p "$build_dir" "$install_dir"
  (
    cd "$build_dir"
    "$src_dir/configure" \
      --prefix="$install_dir" \
      "${CONFIGURE_FLAGS[@]}" \
      CFLAGS="$BUILD_CFLAGS"
    # Build only the libraries linked into libngspice.so; skip src/xspice/icm (external
    # .cm codemodels, including GPLv2+ table.cm and GCC 15 implicit-decl issues in
    # xtradev/seegenerator). Omnia loads only libngspice.so without codemodels.
    for sub in "${BUILD_SUBDIRS[@]}"; do
      make -C "$sub" -j"$(nproc)"
    done
    make -C src -j"$(nproc)" libngspice.la
    make -C src install-libLTLIBRARIES
  )
  printf '%s\n' "$BUILD_STAMP" > "$build_stamp_file"
fi

if [ ! -f "$lib_path" ]; then
  lib_path="$(find "$install_dir" -name 'libngspice.so*' -type f | awk 'NR == 1 { print }')"
fi
if [ -z "$lib_path" ] || [ ! -f "$lib_path" ]; then
  echo "ERROR: libngspice.so not found under $install_dir" >&2
  exit 1
fi

echo "==> Auditing dynamic dependencies of $(basename "$lib_path")..."
{
  echo "=== library ==="
  printf '%s bytes  %s\n' "$(stat -c '%s' "$lib_path")" "$(basename "$lib_path")"
  echo "=== build stamp ==="
  printf '%s\n' "$BUILD_STAMP"
  echo "=== ldd ==="
  ldd "$lib_path"
  if command -v readelf >/dev/null 2>&1; then
    echo "=== readelf -d (NEEDED) ==="
    readelf -d "$lib_path" | grep -E 'NEEDED|SONAME|RPATH|RUNPATH' || true
  fi
} | tee "$results/lib-audit-${label}.txt"

# Fail fast if any forbidden third-party GPL/LGPL library was picked up dynamically.
if ldd "$lib_path" | grep -iE 'lib(readline|fftw3|gomp|klu|cholmod|amd|btf|colamd|suitesparse|history|ncurses|edit|X11|tcl)' >/dev/null; then
  echo "ERROR: forbidden optional library linked into $lib_path (SINV11/SD12)" >&2
  exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$here/spike/target}"
bin_dir="$CARGO_TARGET_DIR/release"

echo "==> Building and testing standalone spike crate..."
(
  cd "$here/spike"
  cargo build --locked --release --bins
  NGSPICE_LIB="$lib_path" cargo test --locked --release
)

(
  cd "$bin_dir"
  sha256sum omnia-ngspice-worker bench_ss1
  printf '%s  %s\n' "$(sha256sum "$lib_path" | awk '{print $1}')" "$(basename "$lib_path")"
) > "$results/binaries-${label}.sha256"

snapshot() {
  echo "timestamp: $(date -Is)"
  echo "kernel: $(uname -srmo)"
  [ -r /etc/os-release ] && echo "os: $(. /etc/os-release && echo "$PRETTY_NAME")"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p')"
  echo "governor: $(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort | uniq -c | tr -s ' ' | tr '\n' ';')"
  for s in /sys/class/power_supply/*; do
    [ -r "$s/type" ] || continue
    echo "power_supply $(basename "$s"): type=$(cat "$s/type") online=$(cat "$s/online" 2>/dev/null || echo -) status=$(cat "$s/status" 2>/dev/null || echo -)"
  done
  echo "power_profile: $(powerprofilesctl get 2>/dev/null || echo unknown)"
  echo "memory: $(free -m | awk '/^Mem:/ {print "total_mb="$2" free_mb="$4}')"
  echo "load: $(cat /proc/loadavg)"
  echo "top_cpu_processes (name = first 8 hex digits of SHA-256(name)):"
  # Drain all ps output via awk instead of head so pipefail never aborts on SIGPIPE (141).
  ps -eo comm=,pcpu=,rss= --sort=-pcpu | awk 'NR <= 8 { print }' | while read -r c p r; do
    echo "  proc-$(printf '%s' "$c" | sha256sum | cut -c1-8) pcpu=$p rss_kb=$r"
  done
  echo "rustc: $(rustc --version)"
  echo "gcc: $(gcc --version | awk 'NR == 1 { print }')"
}

snapshot > "$results/conditions-before-${label}.txt"

echo "==> Running SS1 benchmark (reps=${REPS})..."
"$bin_dir/bench_ss1" \
  --lib "$lib_path" \
  --worker "$bin_dir/omnia-ngspice-worker" \
  --out "$results/ss1-${label}" \
  --reps "$REPS" \
  --startup-iters "$STARTUP_ITERS" \
  --stream-iters "$STREAM_ITERS" \
  --cancel-iters "$CANCEL_ITERS" \
  --batch-size "$BATCH_SIZE" | tee "$results/ss1-${label}.txt"

snapshot > "$results/conditions-after-${label}.txt"
echo "==> Done. Results saved to $results"
