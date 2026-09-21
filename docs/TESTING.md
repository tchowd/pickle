# Testing and evaluation

## Recorded development verification

Environment: macOS 26.6 (25G72), Apple Silicon, Swift 6.3 Command Line Tools. Pickle targets macOS 14+. Xcode 27 is installed but its license is not accepted; this project did not accept it. The standalone test runner avoids that dependency.

| Check | Result | Evidence / limit |
|---|---|---|
| Native app compilation and ad-hoc bundle | Passed | `scripts/build-app.sh` |
| Deterministic automated checks | 46 passed, zero failures | `swift run PickleChecks` |
| Native offline app smoke and reopen | Passed | Generates a sample, hides the panel, invokes reopen, and verifies the same session is visible |
| SwiftUI/AppKit UI inspection | Passed for sample, manual paste, disclosure, settings | Real native Accessibility tree and screenshot inspected |
| Native Cloudflare Simplify | Passed | Public treatment sample, resolved model `@cf/meta/llama-3.3-70b-instruct-sd`, 1.5s observed; uncertainty/scope retained |
| Native Cloudflare Expand and follow-up | Passed | Treatment sample expansion (10.9s observed) and a follow-up on the same source; Jev disabled |
| Native Swift bar chart pipeline | Passed live | $10m / $15m source; values 10,000,000 / 15,000,000 USD, 1.36s |
| Native Swift flow pipeline | Passed live after fixes | Full received → validated → saved sequence, 3.52s; [recorded attempts](evidence/native-live-charts.txt) |
| Latest rebuilt native chart renderer | Compile checked; visual QA pending | Keychain authorization dialog interrupted the app retest; final pipeline verified separately via CLI |
| Rebuild Keychain behavior | Manual authorization needed | Ad-hoc rebuild changed identity; normal reads now use noninteractive LAContext, explicit authorization is in Settings |
| CLI Cloudflare Llama baseline | Passed | [Recorded response](evidence/cloudflare-smoke.json) |
| CLI Jev route | Billing blocked | 402 / 2021, insufficient gateway balance; user requested pending status |
| Jev transport contract | Mocked checks passed | Native Cloudflare endpoint/body/envelope/answer validation |
| Newer request beats late older response | Passed with noncooperative mock | Only new response is delivered |
| Cancelled task cannot deliver success/error | Passed with noncooperative mock | No completion callback |
| Source size, output, conversation budgets | Passed with deterministic cases | Source is rejected, not silently truncated |
| Live provider 429/529/timeouts | Not induced | Explicit code paths; no load/rate-limit tests performed |

Tests cover selective evaluator invocation, missing context before generation, explicit limited context, one repair and recheck, repeated/uncertain concerns, service outages, disabled Jev, local-only gates, source fixation for follow-ups, reading-level override, chart evidence/schema/unit/condition checks, malformed probabilities/distributions, native Cloudflare contracts and usage aliases, no requests on client construction, cancellation, and late-result rejection.

## Actual application compatibility

| Application / scenario | Status |
|---|---|
| Pickle manual-paste input | Tested, works |
| Pickle first-use cloud disclosure | Tested, blocks generation until explicit continuation |
| Pickle offline fixed sample | Tested, no cloud client enabled |
| Browser selection (Safari or Chromium) | Pending Pickle Accessibility permission and hands-on test |
| Native editor selection (TextEdit) | Pending Pickle Accessibility permission and hands-on test |
| PDF viewer selection | Not tested |
| Fast/reverse/short/multiline selections | Implemented release-triggered capture; real-app tests pending |
| Global keyboard capture without focus loss | Implemented; real-app permission-gated validation pending |
| Auto menu stays hidden during drag | Implemented mouse-down hide / mouse-up debounce; real-app validation pending |
| Multi-monitor / full-screen spaces | Placement and AppKit behaviors implemented; manual validation pending |
| VoiceOver and keyboard-only settings audit | Controls have accessible names and native keyboard semantics; full audit pending |
| Excluded app / secure field / revoked permission | Local guards implemented; cross-app manual validation pending |

The app requests neither Screen Recording nor browser automation permissions. No source app was edited to make capture work. Do not interpret this table as claiming browser/native-editor compatibility before permission and actual capture tests.

## Jev evaluation fixtures

`Fixtures/jev-evaluation.json` has **29 draft cases**, spanning:

- Compatible numerical comparisons, processes/branches, unsuitable passages, incompatible units, and dates.
- Missing referents, figures/fragments, ambiguous abbreviations, and self-contained pronouns/jargon.
- Lost uncertainty, negation, exceptions, quantities, attribution; faithful simplification.
- Invented technologies/retention/timelines, unlabeled inferences, valid background and labeled hypothetical examples.
- Four passage difficulty levels and a prompt-injection challenge.

**Labels are AI-drafted expectations, not human-reviewed ground truth.** The review fields are intentionally pending. A human must review and correct source, expected answers and rationale, then set `review.status` to `approved` and record reviewer/date. Do not rubber-stamp these as a human-reviewed benchmark.

The live harness uses the *production Swift evaluator banks*, exported by `PickleChecks --export-evaluators`. It refuses to run without approved human labels, requires `--live`, defaults to three requests, and stops on a provider HTTP failure:

```sh
python3 scripts/evaluate-jev.py --live --account YOUR_ACCOUNT_ID --limit 3
```

Do not run until gateway billing is configured and the user elects to resume live Jev testing.

## Evaluation results and what remains unmeasured

| Metric | Result |
|---|---|
| Mock workflow correctness | 46 checks passed; **not model accuracy** |
| Routing accuracy | Not measured on live Jev |
| False-warning / missed-problem rates | Not measured on live Jev |
| Uncertain outcome rate | Policy branches tested with mocks; live rate unknown |
| Repair success rate | Bounded success/failure paths tested with mocks; live rate unknown |
| Added Jev latency | Unknown; billing failure latency is not inference latency |
| Added Jev cost | Unknown; Cloudflare Jev price/account billing must be checked |
| Baseline without Jev | One recorded CLI Llama sample plus native Simplify/Expand/follow-up and fixed chart checks; not a representative study |

The harness records per-case predictions, correct counts, false warnings, missed problems, uncertainty, elapsed time and reported usage. It does not run repairs or invent dollar costs. A real comparison requires the same source/candidate pairs with and without Jev, human quality judgments independent of another model, separate calibration and held-out cases, and bounded generation/repair experiments. Current 0.2/0.8 thresholds are provisional and must not be described as measured confidence-to-error guarantees.

## Remaining macOS authorization

Pickle’s Accessibility permission was not yet granted/confirmed during this session. The user was asked to grant it or leave compatibility testing pending; no answer was received before packaging. A later rebuild also triggered Keychain authorization. The protected SecurityAgent dialog cannot be controlled through the available computer-use tool. The user was asked to handle it themselves. Do not claim these authorizations or browser/native-editor capture tests are complete.

The final build avoids prompting in ordinary credential reads. Use **Settings → Authorize saved credential…** to explicitly ask macOS for access; that request runs off the UI thread. If an old build is still waiting in a Keychain dialog, cancel/complete that dialog and reopen the new build.

## Launch/reopen regression

A background instance could remain running without displaying a reader when opened again. The app now handles the macOS reopen event and presents the reader on every normal launch. Reopen preserves the current session. The native smoke test covers hide/reopen; the rebuilt app was launched and its visible reader window verified through the real macOS Accessibility tree. The build script now stages a fresh app bundle before replacing the old one, avoiding writes to a running executable.

## Double Option action bar

Three deterministic shortcut checks cover two clean taps, slow taps/holds, and interruptions by typing/modifiers or policy changes. The native smoke also checks that explicit invocation immediately imports the new snapshot and pre-fills its source text without starting generation, then choosing Simplify opens the new snapshot and result. A real native UI preview confirmed all three buttons and clicked Simplify through to the offline sample result. Cross-app physical double-Option invocation remains permission-gated; it is not covered by the sample preview.

## Automatic selection import

Double Option now commits the captured snapshot before displaying the chooser and previews the captured text. The native smoke verifies matching snapshot ID and input text, no result/request before choosing, and the action-to-result path. All 46 checks pass. macOS still reported the latest rebuilt app as untrusted despite an enabled Accessibility switch; toggling the stale entry did not resolve it. Re-adding the current app is pending user assistance. No clipboard synthesis or cloud requests were added.

## Diagram compatibility regression

44 deterministic checks pass. The structured-output regression selects Llama 3.2 1B as writer, verifies the outgoing diagram endpoint uses 70B, and validates returned quantities. A separate check verifies prose still uses 1B. Live production Swift pipeline tests passed with 1B configured: bar 1.33 seconds, full received → validated → saved flow 3.18 seconds. Results are in `evidence/diagram-model-routing.txt`. These are two public fixtures, not a latency benchmark; Jev was not tested.

## Bottom-centered inline floater

All 44 core checks pass. The native smoke verifies window identity is unchanged from chooser to result, the bottom edge and horizontal center remain fixed as it expands, the separate reader stays hidden, the source is imported before generation, and reopen returns to the same floater. The native preview was inspected via Accessibility and screenshot after Simplify; the response, source actions, Copy/Retry, and follow-up field rendered inside the rounded floater. No live model calls were needed for this presentation change.

## Funded Jev integration (September 18)

Browser review confirmed $10.00 AI Gateway credits. A direct public sample returned HTTP 200 with Unified billing and a Completed-job response. The production Swift decoder now supports this response envelope and rejects incomplete jobs. All 46 checks pass. A full live simplification through Llama 1B and Jev returned “Checked against your selection” in 2.33 seconds; see `evidence/jev-live-pipeline.txt` and `evidence/jev-after-credit-check.json`. Prior billing-blocked and pending entries above are historical. Human-reviewed model accuracy evaluation remains pending.


## Control + Option shortcut

The default shortcut is Control + Option alone, pressed together and released. The recognizer verifies either modifier order, single-modifier rejection, one invocation per release, and cancellation by other keys/modifiers, mouse input, or pause. Conventional Control + Option + letter shortcuts do not open the chooser when this mode is active. Tab interception and deferred input replay have been removed. macOS permission changes are being handled by the user.
