from pumphound_ml.replay.runner import PaperEngine, run_replay
from pumphound_ml.types import WSOL_MINT


def test_paper_invariants():
    report = run_replay("fixtures/replay/manifest.json")
    assert report.ledger_balanced
    assert report.reservation_violations == 0
    assert report.lot_violations == 0
    assert report.fill_count >= 1


def test_gross_input_equals_reserve_plus_fees():
    engine = PaperEngine(seed=7)
    engine.fund(WSOL_MINT, 1_000_000_000, "1970-01-01T00:00:00.000000Z")
    engine.reserve("ord-1", WSOL_MINT, 100_000_000)
    fill = engine.fill_buy(
        order_id="ord-1",
        mint="MintA11111111111111111111111111111111111111",
        quote_mint=WSOL_MINT,
        gross=100_000_000,
        vb=1_073_000_000_000_000,
        vq=30_000_000_000,
        at="2026-09-01T00:02:00.000000Z",
        slot=1100,
        min_out=1,
    )
    assert fill is not None
    assert fill["reserve_input"] + fill["fees"] == fill["input_atomic"]
    assert engine.ledger_balanced()
    quote_moves = [m for m in engine.movements if m["mint"] == WSOL_MINT and m["category"] != "initial_funding"]
    assert sum(m["signed_atomic"] for m in quote_moves) == -100_000_000


def test_no_partial_fill_and_no_lot_overconsume():
    engine = PaperEngine(seed=7)
    engine.fund(WSOL_MINT, 1_000_000_000, "1970-01-01T00:00:00.000000Z")
    engine.reserve("ord-1", WSOL_MINT, 100_000_000)
    buy = engine.fill_buy(
        order_id="ord-1",
        mint="MintA11111111111111111111111111111111111111",
        quote_mint=WSOL_MINT,
        gross=100_000_000,
        vb=1_073_000_000_000_000,
        vq=30_000_000_000,
        at="2026-09-01T00:02:00.000000Z",
        slot=1100,
        min_out=1,
    )
    held = sum(lot.remaining for lot in engine.lots)
    engine.reserve("ord-2", buy["output_mint"], held + 1)
    sold = engine.fill_sell(
        order_id="ord-2",
        mint=buy["output_mint"],
        quote_mint=WSOL_MINT,
        base_in=held + 1,
        vb=1_073_000_000_000_000,
        vq=30_000_000_000,
        at="2026-09-01T00:03:00.000000Z",
        slot=1200,
    )
    assert sold is None
    assert engine.lot_violations == 1
    assert engine.ledger_balanced()
