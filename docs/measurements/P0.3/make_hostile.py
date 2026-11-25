"""Write the P0.3 hostile SPICE fixtures byte for byte.

Run once from this directory: python -I make_hostile.py
The fixtures are committed; this script documents how the non-text ones were made.
"""

import os

HEADER = "* SPDX-License-Identifier: CC0-1.0\n* Hostile fixture for the Omnia P0.3 spike: must be refused.\n"
BS = chr(92)  # backslash

FILES = {
    "h01-control.lib": ".subckt EVIL a b\nR1 a b 1k\n.ends EVIL\n.control\nshell rm -rf /\n.endc\n",
    "h02-include-parent.lib": '.include "../outside.lib"\n.model DX D(IS=1e-14)\n',
    "h03-include-absolute.lib": '.lib "C:/models/corners.lib" TT\n.model DX D(IS=1e-14)\n',
    "h04-osdi.lib": "pre_osdi device.osdi\n.model DX D(IS=1e-14)\n",
    "h06-unterminated.lib": ".subckt OPEN a b\nR1 a b 1k\n",
    "h07-include-cycle.lib": '.include "h07-include-cycle.lib"\n.model DX D(IS=1e-14)\n',
    "h08-include-missing.lib": '.include "inc/not-there.lib"\n.model DX D(IS=1e-14)\n',
    "h09-ends-mismatch.lib": ".subckt ONE a b\nR1 a b 1k\n.ends TWO\n",
    "h10-codemodel.lib": "codemodel extra.cm\n.model DX D(IS=1e-14)\n",
    "h11-unresolved-model.lib": ".subckt NEEDS_MODEL a k\nD1 a k DMISSING\n.ends NEEDS_MODEL\n",
    "h12-include-unc.lib": '.include "' + BS * 2 + "server" + BS + "share" + BS + 'x.lib"\n.model DX D(IS=1e-14)\n',
    "h13-csdf.lib": ".csdf\n.model DX D(IS=1e-14)\n",
    "h14-system.lib": "system calc\n.model DX D(IS=1e-14)\n",
    "h15-orphan-continuation.lib": "+ IS=1e-14\n.model DX D(IS=1e-14)\n",
}


def main() -> None:
    os.makedirs("hostile", exist_ok=True)
    for name, body in FILES.items():
        with open(os.path.join("hostile", name), "wb") as f:
            f.write((HEADER + body).encode("ascii"))
    # A Latin-1 micro sign (0xB5) in a comment: not UTF-8, refused under PD10.
    with open(os.path.join("hostile", "h05-latin1.lib"), "wb") as f:
        f.write(HEADER.encode("ascii") + b"* bias current 10 \xb5A\n.model DX D(IS=1e-14)\n")


if __name__ == "__main__":
    main()
