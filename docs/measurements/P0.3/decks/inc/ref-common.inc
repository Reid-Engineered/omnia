* ref-common.inc - shared core for the generic shunt reference
* SPDX-License-Identifier: CC0-1.0
* Written for the Omnia P0.3 spike. Generic parameters, no vendor data.
.subckt SHUNT_CORE K A REF
EREF n 0 REF A 1000
GOUT K A VALUE = { 1m*max(V(n)-2.5,0) }
DREV A K DSHUNT
.ends SHUNT_CORE
.model DSHUNT D(IS=1e-14 RS=0.3)
.model DSHUNT_UNUSED D(IS=1e-12)
