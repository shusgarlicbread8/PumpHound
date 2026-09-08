# PUMPHOUND — PRODUCT TRUTH & AUTONOMOUS LOOP COMPLETION

You are the principal engineer responsible for completing the next PumpHound milestone.

This is NOT a greenfield build.

This is NOT a UI redesign exercise.

This is NOT another infrastructure expansion.

This is NOT a request for a roadmap.

You are working against an existing operating PumpHound system containing:

- live Solana Pump/PumpSwap ingestion;
- paper trading;
- multiple experiment books;
- ALPHA-LOOP-20260908;
- ALPHA-LEARN;
- model registry;
- champion/challenger/shadow models;
- label maturation;
- Hot Set enrichment;
- wallet/creator profiles;
- graph evidence;
- Research;
- Replay infrastructure;
- Tauri desktop UI.

Your task is to make the existing system TRUTHFULLY expose and prove the complete autonomous loop:

MARKET EVENT
→ OBSERVATION
→ FEATURES
→ MODEL / INTELLIGENCE
→ STRATEGY DECISION
→ PAPER ORDER
→ ARRIVAL QUOTE
→ FILL
→ POSITION
→ EXIT DECISION
→ EXIT FILL
→ REALIZED P&L
→ MATURED LABELS
→ LEARNING

The core acceptance principle is:

> IF PUMPHOUND SAYS SOMETHING HAPPENED, THE USER MUST BE ABLE TO TRACE IT TO THE UNDERLYING EVIDENCE.

Do not add major new intelligence until this is true.

==================================================
0. OPERATING PHILOSOPHY
==================================================

Treat this run as a forensic engineering + implementation task.

DO NOT trust:

- previous agent completion reports;
- UI labels;
- comments;
- assumptions in this prompt;
- counters merely because they look plausible.

The authority hierarchy is:

1. persisted source-of-truth data;
2. actual runtime behavior;
3. executable tests and invariants;
4. API responses;
5. UI presentation;
6. documentation / previous agent claims.

If these disagree:

investigate.

Do not choose whichever answer makes the product look best.

Make them reconcile.

NEVER solve contradictions by hiding them in the UI.

==================================================
1. ABSOLUTE INVARIANTS
==================================================

Do not reinterpret or mutate:

ALPHA-LOOP-20260908

It remains frozen.

Do not overwrite historical:

- experiment semantics;
- model versions;
- policy versions;
- decisions;
- orders;
- fills;
- positions;
- outcomes;
- evaluation records.

New corrective infrastructure may reference historical data but must not rewrite history unless there is an explicitly proven data-corruption repair requiring migration.

If such corruption exists:

document it,
prove it,
migrate deliberately,
preserve an audit trail.

ALPHA-LEARN remains the adaptive research track.

No live-money signing or execution is permitted.

PAPER ONLY.

==================================================
2. FIRST ACTION — UNDERSTAND THE REAL SYSTEM
==================================================

Before changing architecture:

inspect the repository and running system.

Map the actual path:

Solana event
→ decoded event
→ launch
→ observation
→ snapshot
→ feature vector
→ prediction
→ strategy decision
→ paper order
→ fill
→ position
→ mark
→ exit
→ P&L
→ labels
→ research metrics
→ UI

Identify the actual:

- database tables;
- primary keys;
- foreign keys;
- IDs;
- strategy identifiers;
- experiment identifiers;
- paper book identifiers;
- model versions;
- policy versions;
- API endpoints;
- UI consumers.

Do not assume symbols/names in this specification exactly match the code.

Build an internal truth map first.

==================================================
3. REPRODUCE THE CURRENT CONTRADICTIONS
==================================================

Before fixing them, reproduce them.

The currently observed product exhibits contradictions such as:

HOME:
64 open positions
714 closed today

while Trade/Positions may show:
0 positions

because a particular book such as HEURISTIC_V1 is selected.

Other observed contradictions include:

- strategy pages reporting N=0 while hundreds of trades exist elsewhere;
- model registry containing a champion while Radar says MODEL NOT TRAINED;
- Home aggregate P&L not obviously matching individual book/ledger values;
- closed positions existing without intuitive Replay access;
- generic model readiness contradicting per-target readiness;
- quarantine lifetime count shown without meaningful rate/context;
- Research diagnostics not directly answering what is working.

Reproduce the actual current versions of these contradictions.

Some numbers above may have changed since this prompt was written.

Use CURRENT state.

Do not code against stale example numbers.

==================================================
4. CREATE THE PUMPHOUND TRUTH CONTRACT
==================================================

Define one canonical product-domain contract across:

Home
Radar
Investigation
Trade
Strategies
Models
Research
Replay
Health

Do not let each screen independently reinterpret storage.

The contract must define canonical semantics for at least:

experiment
book
strategy
strategy version
policy version
model bundle/version
candidate
trade trace
paper order
fill
position
closed position
realized P&L
unrealized P&L
fees
model prediction
feature snapshot
label
market-data freshness
trading readiness

Prefer existing domain abstractions if correct.

Do not create unnecessary duplicate layers.

==================================================
5. CANONICAL IDENTITY / ATTRIBUTION
==================================================

Every autonomous action must be attributable.

Establish or verify a canonical hierarchy resembling:

experiment_id
book_id
strategy_version
policy_version
model_versions
portfolio_id
candidate_id
trade_trace_id

Do not invent new IDs if equivalent durable identities already exist.

Create missing ones only when genuinely required.

Every:

decision
order
fill
position
exit
outcome

must be attributable to the correct experiment/book/strategy.

==================================================
6. TRADE TRACE
==================================================

Implement or formalize an end-to-end durable Trade Trace.

Each candidate/opportunity that reaches strategy evaluation should be traceable through applicable events:

launch
snapshot
feature vector
model prediction(s)
wallet evidence
creator evidence
graph evidence
AI advisory
strategy decision
order
arrival quote
fill
position
mark updates
exit decisions
partial exit(s)
final exit
realized P&L
label maturation

A trace must preserve event timestamps.

It must make clear what information existed at each point in time.

NEVER use future evidence when reconstructing a historical decision.

The trace must distinguish:

OBSERVED LATER

from:

KNOWN AT DECISION TIME

This is essential for avoiding temporal leakage.

==================================================
7. CROSS-SCREEN RECONCILIATION
==================================================

Create automated invariants.

Examples:

HOME open positions
==
ledger open positions for exactly the same active-book scope

HOME closed today
==
closed positions for the defined today/window/scope

strategy.closed_n
==
closed ledger positions attributable to that exact strategy version/book

strategy realized P&L
==
ledger realized P&L for the same scope

order
→ fill
→ position

must maintain canonical trace linkage

closed position
→ trade trace

must exist

model prediction
→ exact model version
→ exact feature snapshot

must exist

If values legitimately differ because their scopes differ:

the scope must be explicit in the API and UI.

No hidden filtering.

==================================================
8. TRADE DEFAULT = ALL ACTIVE BOOKS
==================================================

The main Trade experience must default to:

ALL ACTIVE BOOKS

not HEURISTIC,
not FULL,
not whichever option happens to be first.

The user must immediately see the same aggregate positions represented by Home.

Trade should expose filters for:

- all books;
- experiment;
- strategy;
- state;
- time window.

The aggregate counters must change consistently when filters change.

==================================================
9. TRADE EXPERIENCE
==================================================

Primary Trade navigation should emphasize autonomous activity.

Target structure:

TRADE

Overview
Positions
Orders
History
Activity

Manual paper-order placement may remain available but should be secondary.

PumpHound is an autonomous paper system.

The primary experience should answer:

WHAT DID PUMPHOUND DO?

==================================================
10. AUTONOMOUS ACTIVITY FEED
==================================================

Create a durable or correctly reconstructed activity feed.

Events may include:

CANDIDATE
MODEL_PREDICTION
STRATEGY_DECISION
ORDER_CREATED
ORDER_ARRIVED
FILL
POSITION_OPENED
MODEL_UPDATE
RISK_UPDATE
PARTIAL_EXIT
EXIT_DECISION
EXIT_FILL
POSITION_CLOSED
LABEL_MATURED

Each activity row should link to:

token
trace
strategy
book
position/order where applicable

Filters:

All
Decisions
Orders
Fills
Positions
Risk
Learning

Do not fabricate historical events that were never persisted.

If older trades lack specific evidence, show that honestly.

==================================================
11. CLOSED TRADE → REPLAY
==================================================

Every sufficiently evidenced closed trade must expose:

REPLAY THIS TRADE

Replay should reconstruct the trade using the durable trace and historical observations.

At minimum show:

first observation
feature snapshots
predictions
strategy decisions
order
arrival quote
fill
position events
risk/exit signals
partial exits
final exit
realized outcome

Overlay on the market timeline where data supports it:

price
buys/sells
entry
partial exits
final exit
momentum
adverse
MFE estimate
exit hazard
material graph events
material AI events

Most important:

display what was knowable at the historical virtual time.

No future leakage.

If a replay cannot be reconstructed:

DO NOT show NOT_FOUND as the final product experience.

Show:

REPLAY INCOMPLETE

and list exact missing evidence.

For example:

missing executable trade amounts
slots X-Y

or:

historical feature snapshot unavailable

==================================================
12. MODEL STATE LANGUAGE
==================================================

Remove misleading generic:

MODEL NOT TRAINED

where it conflates unrelated targets.

Every target has its own state.

Examples:

SHORT MOMENTUM 2M
Champion
score 0.71

ADVERSE MOVE 2M
Shadow
score 0.14

EXPECTED MFE 5M
Challenger
score unavailable

EXIT HAZARD 60S
Shadow
score 0.316

GRADUATION 6H
NO MODEL

SEVERE COLLAPSE 1H
NO MODEL

Use explicit availability states:

NO_MODEL

MODEL_AVAILABLE_TOKEN_NOT_SCOREABLE

FEATURES_NOT_READY

OOD

PREDICTION_WITHHELD

PREDICTION_AVAILABLE

MODEL_ERROR

Never treat these as equivalent.

==================================================
13. EXPLAIN UNAVAILABLE PREDICTIONS
==================================================

Investigate must explain WHY a prediction is unavailable.

Examples:

Waiting for 30-second return feature
Expected availability in ~8s

or:

Token OOD
distance / reason ...

or:

Required feature:
independent_buyers_10s
missing

or:

No champion model for this target

Do not display bare:

unavailable

when the backend knows the reason.

==================================================
14. CHAMPION CONTROL LOOP — P0
==================================================

Investigate why:

CHAMPION_SHORT_HORIZON_V1

has existed with zero or unexpectedly low completed trades while Explorer/other books trade.

DO NOT guess.

Build a measurable champion funnel.

Conceptually:

snapshots seen
predictions available
familiarity passed
data-quality passed
momentum gate passed
adverse gate passed
combined gates passed
liquidity passed
freshness passed
capacity available
decision BUY
order created
order arrived
fill
position opened

Record counts and rejection reasons.

The system should answer:

WHY HAS THE CHAMPION NOT TRADED?

without manually reading source code.

If there is a wiring bug:

fix the root cause.

If policy legitimately rejects everything:

prove it with the funnel.

If readiness blocks entries:

show the exact gate.

==================================================
15. STRATEGY STATUS SEMANTICS
==================================================

Stop using:

VALIDATED

to mean only:

N >= threshold

Separate three independent concepts.

SAMPLE STATUS:

INSUFFICIENT
EARLY
SUFFICIENT

EDGE STATUS:

NEGATIVE
INCONCLUSIVE
PROMISING
POSITIVE_OOS

AUTHORITY:

CONTROL
PAPER_ACTIVE
SHADOW
ADVISORY
RETIRED

Do not invent statistical significance.

Define explicit criteria.

A strategy with N=145 and negative net expectancy might show:

Sample: SUFFICIENT
Edge: NEGATIVE
Authority: SHADOW

That is far more honest than VALIDATED.

==================================================
16. STRATEGY ARENA MUST USE LEDGER TRUTH
==================================================

Strategy metrics must come from canonical trades/fills/positions.

For each strategy/book show:

closed N
open N
gross P&L
fees
net P&L
expectancy
win rate
MFE
MAE
max drawdown
capital/exposure
sample state
edge state
authority

Use correct units.

No raw atomic values in presentation.

==================================================
17. FINANCIAL UNIT INTEGRITY
==================================================

Audit the entire paper domain for unit confusion.

Explicitly distinguish:

lamports
SOL
token atomic units
token display quantity
quote currency
price in SOL
price in USD if available
fees

Do not expose values such as:

509215830

without unit/scale.

Every financial API/display value must have unambiguous semantics.

Where feasible, use typed domain structures rather than anonymous integers.

Test conversion boundaries.

==================================================
18. P&L ATTRIBUTION
==================================================

The INFORMATION_VALUE_EXPLORER result has appeared materially stronger than several other strategies.

Treat this as a research finding requiring audit.

Do not celebrate it.

Do not assume it is alpha.

For each strategy calculate or verify:

gross trading P&L
protocol/creator fees where modeled
network/priority fees where modeled
slippage
price impact
=
net P&L

Also expose:

mean entry size
median entry size
fill rate
reject rate
hold time
median winner
median loser
largest winner
largest loser
MFE
MAE
exposure
number of concurrent positions

Determine whether strategy comparisons use equivalent assumptions.

Identify any:

position-sizing difference
exit-policy difference
capital mismatch
stale mark issue
duplicate exposure
fill-model asymmetry
survivorship/selection issue
accounting artifact

If Explorer still performs strongly after reconciliation, report it as an interesting paper result requiring further validation.

==================================================
19. RESEARCH EXECUTIVE LAYER
==================================================

Keep the existing detailed diagnostic Research view.

Add an executive layer ABOVE it answering:

WHAT IS ACTUALLY WORKING?

At minimum show:

best current paper strategy
N
net P&L
expectancy
drawdown
sample status
edge status

Compare important matched books such as:

Random Control
Explorer
Heuristic
Market ML
Graph/AI
Full

Only make incremental-lift claims when comparisons are scientifically defensible.

Examples:

Market ML vs Control
No positive evidence yet

Full vs Market-only
No measurable incremental lift

Graph contribution
Insufficient matched N

AI contribution
Policy effect NONE

If a comparison is confounded:

say so.

==================================================
20. SEPARATE RESEARCH DOMAINS
==================================================

Do not present incomparable counters as one funnel.

Separate:

CANDIDATE FUNNEL

launches
observed
technically invalid
waiting
strategy evaluated
selected
entered
exited

from:

LEARNING THROUGHPUT

feature snapshots
predictions
60s labels
2m labels
5m labels
training rows

from:

MODEL LIFECYCLE

training runs
challengers
shadow models
promotion candidates
promotions
rejections

Each view needs an explicit time window/scope.

==================================================
21. MODELS — JOIN THE WHOLE TRUTH
==================================================

The Models page must combine:

model registry
training artifact
dataset lineage
offline evaluation
live evaluation
trading relationship

Every champion should have retrievable evaluation metadata.

For each model target show:

TARGET
ROLE
VERSION
STATE
TRAINING DATASET
TRAINING ROWS
FEATURE SET
TRAIN RANGE
VALIDATION RANGE

OFFLINE:
appropriate metrics
confidence intervals where available

LIVE:
prediction N
calibration
missingness
OOD
drift

TRADING:
strategies consuming model
paper decisions
paper trades where applicable

PROMOTION:
gates
status
reason

If a registry row says CHAMPION but there is genuinely no evaluation artifact:

investigate why.

Do not merely print:

No numeric metrics

If the champion state is invalid, repair registry consistency rather than hiding it.

Raw evaluation JSON belongs behind:

VIEW RAW EVALUATION

not across the default page.

==================================================
22. MODEL INPUT QUALITY
==================================================

Model trust must incorporate live input quality.

Establish clear semantics for:

missingness
OOD
drift
schema mismatch
feature freshness

A champion may exist while currently being unsuitable for control.

Support states such as:

MODEL READY
MODEL DEGRADED
DATA QUALITY BLOCKED
SCHEMA MISMATCH
INSUFFICIENT LIVE EVIDENCE

Do not allow a model to appear operationally healthy merely because an artifact exists.

==================================================
23. HOME TRUTHFULNESS
==================================================

Home is the cockpit.

Every headline number must be clickable or inspectable.

Examples:

Open positions
→ Trade filtered to exactly that scope

Closed today
→ History filtered to exactly those trades

Strategy
→ Strategy detail

Learning event
→ relevant model/training record

Opportunity
→ investigation

Health problem
→ Health detail

Avoid dead KPI cards.

Home must explicitly show scope.

Example:

64 OPEN
ALL ACTIVE PAPER BOOKS

rather than an unexplained 64.

==================================================
24. OPPORTUNITY FEED
==================================================

Make opportunities informative.

If prediction available:

show relevant model signals and strategy decisions.

If prediction unavailable:

show why.

Example:

WAITING FOR FEATURES

Momentum:
waiting for 30s window
~8s remaining

Known:
7 independent buyers
curve 34%
flow accelerating

Do not present:

p(mom) —
p(adv) —
mfe —

without explanation.

==================================================
25. MARKET VISUALIZATION
==================================================

Never show a fake-looking flat/zero price chart when executable pricing is unavailable.

Support explicit states:

FULL PRICE CHART

RELATIVE RETURN CHART

INSUFFICIENT EXECUTABLE TAPE

Relative mode may show return from first trustworthy executable quote when absolute valuation is unavailable.

If there is insufficient market data:

say so.

==================================================
26. QUARANTINE / DECODER HEALTH
==================================================

Replace the scary contextless lifetime counter with meaningful operational health.

Show:

recent quarantines
total decoded
recent quarantine rate
HOT markets affected
severity

Allow inspection by:

program
instruction/discriminator
reason
signature
decoder version
first seen
last seen
count

Trading readiness should react to relevant quarantine RATE / IMPACT, not merely lifetime absolute count.

Do not hide the lifetime count; contextualize it.

==================================================
27. HEALTH / READINESS MATRIX
==================================================

Separate:

PROCESS HEALTH

from:

DATA HEALTH

from:

TRADING READINESS.

Example dimensions:

Scout
Hot fetch
Postgres
ClickHouse
API
ML worker
Champion models
AI
Label maturer
Trainer

For each show where meaningful:

process state
freshness
latency
trading impact
reason

The overall state must not say HEALTHY while autonomous entry is blocked.

Prefer semantics such as:

SYSTEM RUNNING
TRADING DEGRADED

or:

SYSTEM RUNNING
AUTONOMOUS ENTRY BLOCKED

==================================================
28. CLOSED TRADE REPLAY IS AN ACCEPTANCE GATE
==================================================

Select multiple actual closed trades from different books.

For each:

prove the user can navigate:

Home / Trade
→ closed trade
→ trace
→ replay
→ strategy
→ model prediction

without manually copying IDs.

Where old historical data prevents a complete replay:

show a precise completeness report.

==================================================
29. UI/API CONTRACT ROBUSTNESS
==================================================

Incomplete live data is normal.

No critical route may crash because:

risk missing
orders missing
positions missing
model output absent
token metadata partial
market quote absent
graph evidence absent
AI unavailable
replay partial

Use runtime validation/normalization according to existing project conventions.

Distinguish:

KNOWN ZERO
UNKNOWN
NOT APPLICABLE
PENDING
STALE
ERROR

Do not collapse these into null/0/— indiscriminately.

==================================================
30. NO MAJOR NEW SUBSYSTEMS
==================================================

Do NOT build:

Raydium
Meteora
GNNs
Jito
real-money signing
mobile app
Telegram
new launchpads
large new historical crawls
another generic MLOps framework

Do not redesign the entire storage layer unless evidence proves it necessary.

Repair and connect what already exists.

==================================================
31. ROOT-CAUSE-FIRST POLICY
==================================================

For every observed contradiction:

1. reproduce it;
2. determine scope;
3. trace source data;
4. identify root cause;
5. write/identify failing invariant/test;
6. implement the smallest correct architectural repair;
7. verify;
8. only then continue.

Do not spray optional chaining or UI conditionals across symptoms.

Do not silence inconsistent counters.

Fix source semantics.

==================================================
32. IMPLEMENTATION ORDER
==================================================

Unless repository dependencies prove another sequence necessary:

PHASE A — TRUTH MAPPING

- map current architecture;
- reproduce contradictions;
- define canonical scopes/IDs;
- define reconciliation invariants.

PHASE B — TRADE / LEDGER RECONCILIATION

- All Books default;
- reconcile Home/Trade/Strategies/Research;
- financial units;
- canonical trade trace;
- activity feed foundation.

PHASE C — CHAMPION CONTROL LOOP

- champion funnel;
- determine why N is zero/low;
- repair actual wiring/gates;
- prove decision→order→fill→position.

PHASE D — REPLAY

- link closed position → trace → replay;
- temporal evidence reconstruction;
- incomplete replay semantics.

PHASE E — MODEL TRUTH

- target-specific model states;
- unavailable reasons;
- model metrics joins;
- input quality;
- champion consistency.

PHASE F — RESEARCH / P&L

- P&L attribution;
- strategy status semantics;
- executive research layer;
- honest matched comparisons.

PHASE G — HEALTH

- readiness matrix;
- quarantine context;
- subsystem reason chains.

PHASE H — PRODUCT POLISH

Only after truth invariants pass:
- visual hierarchy;
- better charts;
- probability bars;
- sparklines;
- timelines;
- navigation polish.

==================================================
33. LIVE VERIFICATION
==================================================

Do not consider compilation sufficient.

Use the real local/dev stack.

Where safe and available:

run live Solana RPC fallback / paper mode.

Exercise the application.

Verify actual database/API values.

Run a meaningful soak period if practical.

During verification gather:

number of launches
number of candidates
book trade counts
open positions
closed positions
fills
orders
realized P&L
model predictions
champion funnel
readiness transitions
quarantine rate

==================================================
34. REQUIRED CROSS-SCREEN TEST
==================================================

Before declaring completion, perform this exact product journey using CURRENT real data.

1.

Open Home.

Record displayed open-position count.

2.

Click/open Trade ALL BOOKS.

Count must reconcile.

3.

Open one actual position.

Verify:

book
strategy version
model version(s)
policy
entry
fill
current mark
decision evidence

4.

Open its Trade Trace.

Verify event linkage.

5.

Close or choose an already closed trade.

Verify realized P&L.

6.

Open Replay.

Verify historical events and model/strategy decisions.

7.

Open Strategy.

Verify that trade contributes to its N/P&L.

8.

Open relevant Model.

Verify exact model artifact/evaluation.

9.

Open Research.

Verify corresponding experiment aggregate.

If any value legitimately differs:

state the exact scope difference visibly.

==================================================
35. REQUIRED AUTOMATED INVARIANTS
==================================================

Add tests appropriate to repository architecture for:

- Home open count vs ledger scope;
- Home closed count vs ledger scope;
- Strategy N reconciliation;
- Strategy P&L reconciliation;
- order/fill/position trace integrity;
- closed position replay linkage;
- exact model version on prediction;
- exact feature snapshot linkage;
- financial unit conversions;
- All Books default aggregation;
- target-specific model state;
- missing-data domain states;
- champion funnel accounting;
- shadow models cannot control;
- frozen ALPHA-LOOP invariant.

Also retain all existing tests.

==================================================
36. COMPLETION STANDARD
==================================================

Do not claim this milestone complete because:

the pages exist

or:

tests compile

or:

the APIs return HTTP 200.

Completion means the autonomous loop is traceable and reconciled.

The release is complete only when:

1. Home/Trade/Strategies/ledger reconcile for the same scopes;

2. Trade defaults to ALL ACTIVE BOOKS;

3. autonomous activity is visible;

4. a real trade can be traced from observation through fill/exit/outcome;

5. closed trades expose usable Replay or precise completeness failures;

6. Champion Short Horizon either trades correctly or has an exact observable rejection funnel proving why it does not;

7. model states are target-specific and truthful;

8. every champion has traceable training/evaluation lineage or registry inconsistency is repaired;

9. Explorer performance has P&L attribution sufficient to audit it;

10. Research says what appears to work without overstating evidence;

11. strategy sample/edge/authority states are separated;

12. financial quantities have correct explicit units;

13. quarantine is contextualized;

14. Health distinguishes process health, data health and trading readiness;

15. critical routes survive partial data;

16. ALPHA-LOOP-20260908 remains frozen;

17. no live-money path is introduced.

==================================================
37. WORK AUTONOMOUSLY
==================================================

Do not repeatedly ask me whether to proceed.

Use repository/runtime evidence.

If an assumption in this specification is wrong:

correct the implementation according to evidence and document the discrepancy.

If one subsystem is externally blocked:

continue unrelated work.

Do not stop after writing a plan.

Inspect.

Run.

Measure.

Implement.

Test.

Open the application where tools permit.

Verify.

Iterate.

==================================================
38. DO NOT TRUST YOURSELF TOO EASILY
==================================================

Before declaring each P0 complete:

try to disprove your own result.

Examples:

If Home says 64 and Trade says 64:

verify they aren't independently wrong in the same way.

Compare against the ledger directly.

If Strategy reports N=145:

query the underlying closed positions.

If P&L reconciles:

recalculate it independently from fills.

If Replay works:

verify no future evidence entered earlier historical states.

If the champion trades:

prove the controlling prediction came from the champion rather than a shadow model.

Evidence before assertion.

==================================================
39. FINAL REPORT
==================================================

When done, produce an evidence-based report.

## Repository state

- git status / branch / commits if git exists;
- if git does not exist, explicitly state this and list changed files/components.

## Contradictions found

For each important contradiction:

EXPECTED
ACTUAL
ROOT CAUSE
FIX
PROOF

## Truth contract

Document the canonical identity/scope rules implemented.

## Reconciliation

Show actual current values for:

Home
Trade
Strategies
Ledger
Research

and demonstrate reconciliation.

## Champion loop

Show the full current funnel from observations → orders/fills.

If the champion still does not trade:

show exactly which gates dominate.

## Trade Trace

Give one concrete real trace ID and summarize its complete lifecycle.

## Replay

Show at least one replayable closed trade and any historical completeness limitations.

## P&L audit

Show strategy-level P&L attribution and explain any material discrepancies fixed.

## Models

Show:

champion
challengers
model metrics
live input quality
strategy consumers

## Research

State only evidence-supported conclusions.

## Health

Show process/data/readiness status and quarantine context.

## Verification

List exact:

tests
builds
commands
queries
runtime journeys

and their actual results.

## Remaining gaps

Separate:

BLOCKERS
DEFERRED
TECHNICAL DEBT

## Next engineering objective

Give exactly ONE highest-value next milestone.

Do not end with:

"the foundation is in place."

Tell me what PumpHound can now demonstrably do.