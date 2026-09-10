"""Regression coverage for pumphound_ml.postcost.dataset_factory's price/return math.

The first live run of this factory reported net_return_300s ~= -0.999999 for every single
successful entry — every trade apparently wiped out. The real cause was a units mismatch:
price_of() computed a raw quote_atomic/base_atomic ratio while entry_price used
execution_price_e9 directly, which is that same ratio scaled by pump_math::mid_price_e9's
1_000_000 * 1_000_000_000 factor (the 1e6 corrects for the base token's 6 decimals vs the quote
asset's 9). Comparing the two without matching scale makes every forward price look ~1e6x
smaller than entry, i.e. a fabricated near-total loss on every row. These tests pin the fix.
"""

from __future__ import annotations

from pumphound_ml.postcost.dataset_factory import compute_forward_labels, price_of


def test_price_of_matches_execution_price_e9_scale():
    # Same reserves pump_math::mid_price_e9 would receive: a token with 6 decimals trading
    # against 9-decimal SOL. mid_price_e9 = quote * 1e6 * 1e9 / base.
    quote_atomic = 5_000_000  # 0.005 SOL
    base_atomic = 100_000_000_000  # 100,000 tokens at 6 decimals
    expected_execution_price_e9 = quote_atomic * 1_000_000 * 1_000_000_000 // base_atomic
    row = {"effective_quote_atomic": quote_atomic, "effective_base_atomic": base_atomic}
    assert price_of(row) == expected_execution_price_e9


def test_unchanged_price_yields_zero_return_not_total_loss():
    # A token whose price never moves must show ~0% return, not ~-100% from a scale mismatch.
    quote_atomic, base_atomic = 5_000_000, 100_000_000_000
    entry_price = price_of({"effective_quote_atomic": quote_atomic, "effective_base_atomic": base_atomic})
    from datetime import datetime, timedelta, timezone

    entry_time = datetime(2026, 1, 1, tzinfo=timezone.utc)
    path = [
        {
            "observed_at": entry_time + timedelta(seconds=10),
            "effective_quote_atomic": quote_atomic,
            "effective_base_atomic": base_atomic,
        }
    ]
    labels = compute_forward_labels(path, entry_price, entry_time + timedelta(seconds=15), entry_time)
    assert labels.net_return_30s is not None
    assert abs(labels.net_return_30s) < 1e-9


def test_doubled_price_yields_plus_one_return():
    quote_atomic, base_atomic = 5_000_000, 100_000_000_000
    entry_price = price_of({"effective_quote_atomic": quote_atomic, "effective_base_atomic": base_atomic})
    from datetime import datetime, timedelta, timezone

    entry_time = datetime(2026, 1, 1, tzinfo=timezone.utc)
    path = [
        {
            "observed_at": entry_time + timedelta(seconds=10),
            "effective_quote_atomic": quote_atomic * 2,
            "effective_base_atomic": base_atomic,
        }
    ]
    labels = compute_forward_labels(path, entry_price, entry_time + timedelta(seconds=15), entry_time)
    assert labels.net_return_30s is not None
    assert abs(labels.net_return_30s - 1.0) < 1e-9
