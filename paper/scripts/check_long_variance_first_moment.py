"""Check exact first-moment identities for the long-variance derivations."""

from dataclasses import dataclass
from fractions import Fraction


@dataclass(frozen=True)
class LongVarianceFirstMomentCase:
    mean_reversion_rate: Fraction
    buy_event_rate: Fraction
    sell_event_rate: Fraction
    buy_event_loading: Fraction
    sell_event_loading: Fraction

    def event_feedback_drift(self) -> Fraction:
        return (
            self.buy_event_loading * self.buy_event_rate
            + self.sell_event_loading * self.sell_event_rate
        )

    def stationary_mean_reversion_level(self) -> Fraction:
        return Fraction(1) - self.event_feedback_drift() / self.mean_reversion_rate

    def unit_mean_drift(self) -> Fraction:
        return self.mean_reversion_rate * (
            self.stationary_mean_reversion_level() - Fraction(1)
        ) + self.event_feedback_drift()

    def first_moment_guard_holds(self) -> bool:
        return self.event_feedback_drift() < self.mean_reversion_rate


VALID_CASES: tuple[LongVarianceFirstMomentCase, ...] = (
    LongVarianceFirstMomentCase(
        mean_reversion_rate=Fraction(4),
        buy_event_rate=Fraction(1),
        sell_event_rate=Fraction(3, 2),
        buy_event_loading=Fraction(1, 8),
        sell_event_loading=Fraction(1, 4),
    ),
    LongVarianceFirstMomentCase(
        mean_reversion_rate=Fraction(3),
        buy_event_rate=Fraction(5, 4),
        sell_event_rate=Fraction(1, 2),
        buy_event_loading=Fraction(1, 5),
        sell_event_loading=Fraction(1, 2),
    ),
)

BOUNDARY_CASE: LongVarianceFirstMomentCase = LongVarianceFirstMomentCase(
    mean_reversion_rate=Fraction(2),
    buy_event_rate=Fraction(3),
    sell_event_rate=Fraction(1),
    buy_event_loading=Fraction(1, 2),
    sell_event_loading=Fraction(1, 2),
)

SUPERCRITICAL_CASE: LongVarianceFirstMomentCase = LongVarianceFirstMomentCase(
    mean_reversion_rate=Fraction(2),
    buy_event_rate=Fraction(4),
    sell_event_rate=Fraction(1),
    buy_event_loading=Fraction(1, 2),
    sell_event_loading=Fraction(1, 2),
)


def check_unit_mean_normalization() -> None:
    for valid_case in VALID_CASES:
        assert valid_case.first_moment_guard_holds()
        assert valid_case.stationary_mean_reversion_level() > 0
        assert valid_case.unit_mean_drift() == 0


def check_first_moment_guard_boundary() -> None:
    assert not BOUNDARY_CASE.first_moment_guard_holds()
    assert BOUNDARY_CASE.stationary_mean_reversion_level() == 0


def check_first_moment_guard_violation() -> None:
    assert not SUPERCRITICAL_CASE.first_moment_guard_holds()
    assert SUPERCRITICAL_CASE.stationary_mean_reversion_level() < 0


def main() -> None:
    check_unit_mean_normalization()
    check_first_moment_guard_boundary()
    check_first_moment_guard_violation()

    print("long-variance first-moment checks passed")


if __name__ == "__main__":
    main()
