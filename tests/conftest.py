from pathlib import Path

import json

import pytest

from janiform.calendar import CalendarRegistry


@pytest.fixture(scope="session")
def project_root() -> Path:
    return Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def python_source_root(project_root: Path) -> Path:
    return project_root / "python"


@pytest.fixture(scope="session")
def calendar_registry_json() -> str:
    weekdays: list[str] = [
        'Monday',
        'Tuesday',
        'Wednesday',
        'Thursday',
        'Friday',
    ]
    regular_sessions: list[dict[str, object]] = [
        {
            'weekday': weekday,
            'windows': [
                {
                    'label': 'regular',
                    'open_time': '09:30',
                    'close_time': '16:00',
                },
            ],
        }
        for weekday in weekdays
    ]
    xtks_regular_sessions: list[dict[str, object]] = [
        {
            'weekday': weekday,
            'windows': [
                {
                    'label': 'regular',
                    'open_time': '09:30',
                    'close_time': '15:00',
                },
            ],
        }
        for weekday in weekdays
    ]
    registry_artifact: dict[str, object] = {
        'schema_version': 'calendar.registry.v1',
        'artifact_date': '2026-01-31',
        'exchanges': [
            {
                'schema_version': 'calendar.v1',
                'exchange_code': 'XNYS',
                'exchange_name': 'New York Stock Exchange',
                'timezone_name': 'America/New_York',
                'rule_metadata': {
                    'holiday_calendar_code': 'US_EQUITIES',
                    'early_close_rule_set': 'US_EQUITIES_EARLY_CLOSE',
                },
                'regular_sessions': regular_sessions,
                'holiday_dates': ['2026-02-03'],
                'early_close_sessions': [
                    {
                        'date': '2026-02-04',
                        'windows': [
                            {
                                'label': 'early_close',
                                'open_time': '09:30',
                                'close_time': '13:00',
                            },
                        ],
                    },
                ],
            },
            {
                'schema_version': 'calendar.v1',
                'exchange_code': 'XTKS',
                'exchange_name': 'Tokyo Stock Exchange',
                'timezone_name': 'Asia/Tokyo',
                'rule_metadata': {
                    'holiday_calendar_code': 'JP_EQUITIES',
                    'early_close_rule_set': None,
                },
                'regular_sessions': xtks_regular_sessions,
                'holiday_dates': [],
                'early_close_sessions': [],
            },
        ],
    }
    return json.dumps(registry_artifact)


@pytest.fixture()
def calendar_registry(calendar_registry_json: str) -> CalendarRegistry:
    return CalendarRegistry(calendar_registry_json)
