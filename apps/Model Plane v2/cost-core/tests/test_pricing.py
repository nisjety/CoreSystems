from decimal import Decimal

from app.pricing import calculate_usd, get_pricing


def test_get_pricing_known_model() -> None:
    inp, outp = get_pricing("claude-sonnet-4-5")
    assert inp == Decimal("3.00")
    assert outp == Decimal("15.00")


def test_get_pricing_prefix_match() -> None:
    inp, outp = get_pricing("claude-sonnet-4-5-20250101")
    assert inp == Decimal("3.00")
    assert outp == Decimal("15.00")


def test_calculate_usd_rounding() -> None:
    cost = calculate_usd("gpt-4o-mini", 1_000_000, 1_000_000)
    assert cost == Decimal("0.75000000")
