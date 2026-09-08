# AI Investigator and Evidence Committee

## Role

Language models investigate and explain structured evidence. They do not calculate reserves, balances, P&L, probabilities or fills, and they never control a transaction path.

PumpHound works without an LLM. If every model provider is offline, live scoring, graph inference, alerts, paper trading and replay continue.

## 1. Provider order

1. Local OpenAI-compatible endpoint through `llama.cpp` or Ollama.
2. Optional SiliconFlow OpenAI-compatible endpoint.
3. No-provider deterministic evidence summary.

Recommended starting local model: a configurable Qwen3-class 14B instruct model in an Apple-silicon-friendly quantization. Treat the exact model and quantization as a deployment choice, not a source-code constant. The provider adapter exposes model ID, context window and structured-output capability.

Cloud prompts receive only the evidence required for the question. Never send secrets, API keys, raw environment data, private notes, or unnecessary wallet history.

## 2. Investigator roles

These are independent prompt profiles over the same tool gateway, not autonomous services:

| Role | Question | Required output |
|---|---|---|
| Launch Quant | What do the calibrated models and path features imply? | probabilities, caveats, feature evidence |
| Wallet Investigator | What plausible structure explains these wallets? | hypotheses, supporting/counter evidence, gaps |
| Creator Investigator | How does point-in-time creator history compare? | history summary and base-rate context |
| Risk Investigator | What can make the apparent edge untradeable? | risk list, cost/data sensitivity, veto rationale |
| Contrarian | What evidence challenges the current consensus? | strongest alternate explanation |
| Committee Synthesizer | How should evidence be summarized for a human? | `WATCH`, `WAIT`, `AVOID`, `PAPER_CANDIDATE`, never live order |

The committee runs only when requested, on high-ranked candidates, or on configured alerts. It must not block the event pipeline.

## 3. Evidence tools

The model can call read-only, schema-validated tools:

- `get_launch_snapshot(launch_id, as_of_slot)`;
- `get_model_predictions(launch_id, cutoff)`;
- `get_wallet_profile(wallet, as_of_slot)`;
- `get_cluster_evidence(cluster_id, version)`;
- `trace_funding_paths(wallet, as_of_slot, max_depth)`;
- `get_creator_history(creator, as_of_slot)`;
- `get_trade_timeline(launch_id, start_slot, end_slot)`;
- `get_cost_sensitivity(launch_id, policy_version)`;
- `resolve_evidence(evidence_ids)`.

No SQL, shell, network-fetch or generic filesystem tool is exposed to the LLM. The application, not the model, enforces time cutoffs and result limits.

## 4. Structured response

All investigator responses validate against a schema equivalent to:

```json
{
  "summary": "string",
  "stance": "WATCH|WAIT|AVOID|PAPER_CANDIDATE",
  "confidence": 0.0,
  "claims": [
    {
      "claim": "string",
      "supporting_evidence_ids": ["string"],
      "counter_evidence_ids": ["string"],
      "uncertainty": "string"
    }
  ],
  "data_gaps": ["string"],
  "suggested_checks": ["string"]
}
```

The gateway rejects unknown evidence IDs, probabilities outside `[0,1]`, live-trade language in machine fields, or an unsupported stance. Narrative claims without evidence appear visually as unverified and cannot affect a paper policy.

## 5. Grounding and injection resistance

- Treat token names, symbols, metadata, websites, socials, transaction memos and fetched page text as untrusted data.
- Wrap untrusted text as quoted evidence with source and truncation metadata.
- Never concatenate untrusted text into system or tool instructions.
- Fetch metadata through a backend proxy with size, MIME, redirect, IP-range and timeout limits.
- Deny private, loopback, link-local and cloud-metadata addresses to prevent SSRF.
- Sanitize markup and never execute embedded scripts.
- Keep provider system prompts versioned and content-hashed.
- Log prompt template ID, model ID, tool calls, evidence IDs, latency and token counts; redact secrets.

## 6. Deterministic facts versus narrative

The UI visually separates:

- **Observed:** decoded on-chain facts.
- **Derived:** deterministic calculations or graph algorithms.
- **Predicted:** model probabilities.
- **Interpreted:** LLM hypotheses.

The LLM may translate `0.78` to “higher than the current base rate,” but cannot recalculate it. If a model probability is absent, it must say unavailable.

## 7. Committee flow

1. Freeze an evidence pack at one `as_of_slot`.
2. Run Launch Quant, Wallet, Creator and Risk profiles independently.
3. Run Contrarian with their structured outputs plus the original evidence IDs.
4. Run the Synthesizer with all outputs and the current policy constraints.
5. Validate schema and evidence references.
6. Persist each response separately with provider/model/prompt versions.
7. Render dissent; do not collapse it into a false unanimous statement.

No agent reads another agent's private chain of thought. Only validated structured outputs are passed forward.

## 8. Latency and budget controls

- Local models: default 15-second timeout for an interactive investigation.
- Cloud models: configurable daily token/cost ceiling and per-request timeout.
- Cache by evidence-pack hash + prompt version + model ID.
- Cancel stale candidate investigations when requested, without affecting ingestion.
- Stream narrative tokens only after structured facts render.
- Degrade to the deterministic evidence summary on timeout or schema failure.

## 9. Model evaluation

Maintain a fixed set of historical evidence packs with expert-authored expected claims, prohibited claims and required uncertainty. Score:

- evidence citation precision;
- unsupported claim rate;
- numeric transcription accuracy;
- recognition of missing data;
- stability across repeated runs;
- resistance to instructions embedded in token metadata;
- disagreement quality from the Contrarian role.

The LLM is promoted separately from predictive models and paper policies.

