# W2.2 overlap during the Orion T4.6 campaign

The owner reported W2.2 cargo builds, tests, Clippy, and a WB3 diagnostic on Orion in the five windows below on 2026-11-23. All times are EST (UTC−05:00). The first T4.6 before-snapshot was 21:02:38.031302.

Invocation intervals use their immediately preceding and following condition timestamps. Snapshot process names and PIDs come from the journal. The owner report identifies the workstream. The snapshot inventories show process names and PIDs only. They leave the cause of individual budget misses undetermined.

## Invocation intervals intersecting the reported windows

| Owner-reported window | Invocation | Before | After | Intersection (s) |
| --- | --- | --- | --- | ---: |
| 21:02:09–21:02:20 | None | | | 0 |
| 21:06:05–21:08:28 | `s100/10k-zoom-2` | 21:05:55.098403 | 21:06:16.753206 | 11.753206 |
| 21:06:05–21:08:28 | `s100/hit-10k-2` | 21:06:16.914834 | 21:06:17.230390 | 0.315556 |
| 21:06:05–21:08:28 | `s100/start-1k-2` | 21:06:17.428671 | 21:06:26.556404 | 9.127733 |
| 21:06:05–21:08:28 | `s100/start-empty-2` | 21:06:26.820281 | 21:06:33.830564 | 7.010283 |
| 21:06:05–21:08:28 | `s100/1k-pan-3` | 21:06:34.155214 | 21:06:55.734694 | 21.579480 |
| 21:06:05–21:08:28 | `s100/1k-zoom-3` | 21:06:56.041943 | 21:07:18.806528 | 22.764585 |
| 21:06:05–21:08:28 | `s100/10k-pan-3` | 21:07:19.180154 | 21:07:41.056205 | 21.876051 |
| 21:06:05–21:08:28 | `s100/10k-zoom-3` | 21:07:41.417165 | 21:08:05.361621 | 23.944456 |
| 21:06:05–21:08:28 | `s100/hit-10k-3` | 21:08:05.551241 | 21:08:05.833156 | 0.281915 |
| 21:06:05–21:08:28 | `s100/start-empty-3` | 21:08:06.007661 | 21:08:11.253229 | 5.245568 |
| 21:06:05–21:08:28 | `s100/start-1k-3` | 21:08:11.451659 | 21:08:17.714618 | 6.262959 |
| 21:06:05–21:08:28 | `s100/10k-hover-3` | 21:08:17.903968 | 21:08:38.982446 | 10.096032 |
| 21:08:37–21:08:52 | `s100/10k-hover-3` | 21:08:17.903968 | 21:08:38.982446 | 1.982446 |
| 21:08:37–21:08:52 | `s100/1k-zoom-4` | 21:08:39.168396 | 21:09:01.403202 | 12.831604 |
| 21:09:35–21:10:37 | `s100/10k-zoom-4` | 21:09:22.679672 | 21:09:45.061185 | 10.061185 |
| 21:09:35–21:10:37 | `s100/hit-10k-4` | 21:09:45.373136 | 21:09:45.854756 | 0.481620 |
| 21:09:35–21:10:37 | `s100/start-1k-4` | 21:09:46.201790 | 21:09:56.133781 | 9.931991 |
| 21:09:35–21:10:37 | `s100/start-empty-4` | 21:09:56.459164 | 21:10:05.062279 | 8.603115 |
| 21:09:35–21:10:37 | `s100/10k-hover-4` | 21:10:05.406688 | 21:10:27.922939 | 22.516251 |
| 21:09:35–21:10:37 | `s100/1k-pan-4` | 21:10:28.287684 | 21:10:49.530875 | 8.712316 |
| 21:10:43–21:12:12 | `s100/1k-pan-4` | 21:10:28.287684 | 21:10:49.530875 | 6.530875 |
| 21:10:43–21:12:12 | `s100/10k-pan-5` | 21:10:49.729372 | 21:11:11.447978 | 21.718606 |
| 21:10:43–21:12:12 | `s100/10k-zoom-5` | 21:11:11.806223 | 21:11:37.341388 | 25.535165 |
| 21:10:43–21:12:12 | `s100/hit-10k-5` | 21:11:37.639814 | 21:11:38.240613 | 0.600799 |
| 21:10:43–21:12:12 | `s100/start-empty-5` | 21:11:38.576189 | 21:11:47.180743 | 8.604554 |
| 21:10:43–21:12:12 | `s100/start-1k-5` | 21:11:47.391948 | 21:11:54.040448 | 6.648500 |
| 21:10:43–21:12:12 | `s100/10k-hover-5` | 21:11:54.250707 | 21:12:15.253818 | 17.749293 |

The four intersecting windows cover 25 distinct invocations in s100 repetitions 2–5. The first reported work window ended before the first invocation.

## Exact before and after snapshot matches in s100 repetitions 1–5

Of the 80 snapshots inspected, 46 fall inside a reported window. These rows give every such snapshot.

| Owner-reported window | Invocation | Phase | Snapshot time | Process matches (PID and name) |
| --- | --- | --- | --- | --- |
| 21:06:05–21:08:28 | `s100/10k-zoom-2` | after | 21:06:16.753206 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/hit-10k-2` | before | 21:06:16.914834 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/hit-10k-2` | after | 21:06:17.230390 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/start-1k-2` | before | 21:06:17.428671 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/start-1k-2` | after | 21:06:26.556404 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/start-empty-2` | before | 21:06:26.820281 | `84891 cargo`, `84901 rustc` |
| 21:06:05–21:08:28 | `s100/start-empty-2` | after | 21:06:33.830564 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/1k-pan-3` | before | 21:06:34.155214 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/1k-pan-3` | after | 21:06:55.734694 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/1k-zoom-3` | before | 21:06:56.041943 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/1k-zoom-3` | after | 21:07:18.806528 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/10k-pan-3` | before | 21:07:19.180154 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/10k-pan-3` | after | 21:07:41.056205 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/10k-zoom-3` | before | 21:07:41.417165 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/10k-zoom-3` | after | 21:08:05.361621 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/hit-10k-3` | before | 21:08:05.551241 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/hit-10k-3` | after | 21:08:05.833156 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/start-empty-3` | before | 21:08:06.007661 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/start-empty-3` | after | 21:08:11.253229 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/start-1k-3` | before | 21:08:11.451659 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/start-1k-3` | after | 21:08:17.714618 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:06:05–21:08:28 | `s100/10k-hover-3` | before | 21:08:17.903968 | `84891 cargo`, `86178 omnia_doc-2981c` |
| 21:08:37–21:08:52 | `s100/10k-hover-3` | after | 21:08:38.982446 | `87213 cargo-clippy`, `87222 cargo` |
| 21:08:37–21:08:52 | `s100/1k-zoom-4` | before | 21:08:39.168396 | `87213 cargo-clippy`, `87222 cargo` |
| 21:09:35–21:10:37 | `s100/10k-zoom-4` | after | 21:09:45.061185 | `87650 cargo-clippy`, `87659 cargo`, `87707 rustc`, `87723 rustc`, `87733 rustc`, `87738 rustc` |
| 21:09:35–21:10:37 | `s100/hit-10k-4` | before | 21:09:45.373136 | `87650 cargo-clippy`, `87659 cargo`, `87707 rustc`, `87723 rustc`, `87733 rustc`, `87738 rustc` |
| 21:09:35–21:10:37 | `s100/hit-10k-4` | after | 21:09:45.854756 | `87650 cargo-clippy`, `87659 cargo`, `87707 rustc`, `87723 rustc`, `87733 rustc`, `87778 rustc` |
| 21:09:35–21:10:37 | `s100/start-1k-4` | before | 21:09:46.201790 | `87650 cargo-clippy`, `87659 cargo`, `87707 rustc`, `87723 rustc`, `87733 rustc`, `87778 rustc` |
| 21:09:35–21:10:37 | `s100/start-1k-4` | after | 21:09:56.133781 | `87650 cargo-clippy`, `87659 cargo`, `87966 rustc`, `87997 rustc`, `88039 rustc`, `88052 rustc` |
| 21:09:35–21:10:37 | `s100/start-empty-4` | before | 21:09:56.459164 | `87650 cargo-clippy`, `87659 cargo`, `87966 rustc`, `87997 rustc`, `88039 rustc`, `88052 rustc` |
| 21:09:35–21:10:37 | `s100/start-empty-4` | after | 21:10:05.062279 | `87650 cargo-clippy`, `87659 cargo`, `87997 rustc`, `88039 rustc`, `88325 rustc`, `88356 rustc` |
| 21:09:35–21:10:37 | `s100/10k-hover-4` | before | 21:10:05.406688 | `87650 cargo-clippy`, `87659 cargo`, `87997 rustc`, `88039 rustc`, `88325 rustc`, `88356 rustc` |
| 21:09:35–21:10:37 | `s100/10k-hover-4` | after | 21:10:27.922939 | `87650 cargo-clippy`, `87659 cargo`, `88039 rustc`, `88703 clippy-driver`, `88726 clippy-driver`, `88728 clippy-driver` |
| 21:09:35–21:10:37 | `s100/1k-pan-4` | before | 21:10:28.287684 | `87650 cargo-clippy`, `87659 cargo`, `88039 rustc`, `88728 clippy-driver`, `88741 clippy-driver` |
| 21:10:43–21:12:12 | `s100/1k-pan-4` | after | 21:10:49.530875 | `88944 cargo-clippy`, `88953 cargo`, `88958 clippy-driver` |
| 21:10:43–21:12:12 | `s100/10k-pan-5` | before | 21:10:49.729372 | `88944 cargo-clippy`, `88953 cargo`, `88958 clippy-driver` |
| 21:10:43–21:12:12 | `s100/10k-pan-5` | after | 21:11:11.447978 | `88943 cargo`, `89054 rustc`, `89084 rustc`, `89100 rustc` |
| 21:10:43–21:12:12 | `s100/10k-zoom-5` | before | 21:11:11.806223 | `88943 cargo`, `89054 rustc`, `89084 rustc`, `89100 rustc` |
| 21:10:43–21:12:12 | `s100/10k-zoom-5` | after | 21:11:37.341388 | `88943 cargo`, `89381 rustc`, `89382 rustc` |
| 21:10:43–21:12:12 | `s100/hit-10k-5` | before | 21:11:37.639814 | `88943 cargo`, `89381 rustc`, `89382 rustc` |
| 21:10:43–21:12:12 | `s100/hit-10k-5` | after | 21:11:38.240613 | `88943 cargo`, `89381 rustc`, `89382 rustc` |
| 21:10:43–21:12:12 | `s100/start-empty-5` | before | 21:11:38.576189 | `88943 cargo`, `89381 rustc`, `89382 rustc` |
| 21:10:43–21:12:12 | `s100/start-empty-5` | after | 21:11:47.180743 | `88943 cargo`, `89381 rustc`, `89762 cc`, `89764 rust-lld` |
| 21:10:43–21:12:12 | `s100/start-1k-5` | before | 21:11:47.391948 | `88943 cargo`, `89791 follow_command-` |
| 21:10:43–21:12:12 | `s100/start-1k-5` | after | 21:11:54.040448 | `88943 cargo`, `89791 follow_command-` |
| 21:10:43–21:12:12 | `s100/10k-hover-5` | before | 21:11:54.250707 | `88943 cargo`, `89791 follow_command-` |

The other inspected snapshots contained zero matches for these process names. The complete campaign is non-certifying under canvas plan §2.1.
