#!/usr/bin/env python3
"""Generate dashboards/thermostats.json for ecobee-exporter."""

from __future__ import annotations

import json
from pathlib import Path

DS = {"type": "prometheus", "uid": "${datasource}"}


def prom(expr: str, **extra) -> dict:
    spec = {
        "editorMode": "code",
        "expr": expr,
        "legendFormat": extra.get("legendFormat", ""),
        "range": extra.get("range", True),
    }
    if extra.get("interval"):
        spec["interval"] = extra["interval"]
    if extra.get("instant"):
        spec["instant"] = True
        spec["range"] = False
    if extra.get("format"):
        spec["format"] = extra["format"]
    return {
        "datasource": DS,
        "group": "prometheus",
        "kind": "DataQuery",
        "spec": spec,
        "version": "v0",
    }


def panel_query(ref_id: str, expr: str, hidden: bool = False, **extra) -> dict:
    return {
        "kind": "PanelQuery",
        "spec": {
            "hidden": hidden,
            "query": prom(expr, **extra),
            "refId": ref_id,
        },
    }


def timeseries_panel(
    pid: int,
    title: str,
    queries: list[dict],
    overrides: list | None = None,
    interval: str | None = None,
) -> dict:
    query_options = {"interval": interval} if interval else {}
    return {
        "kind": "Panel",
        "spec": {
            "data": {
                "kind": "QueryGroup",
                "spec": {
                    "queries": queries,
                    "queryOptions": query_options,
                    "transformations": [],
                },
            },
            "description": "",
            "id": pid,
            "links": [],
            "title": title,
            "vizConfig": {
                "group": "timeseries",
                "kind": "VizConfig",
                "spec": {
                    "fieldConfig": {
                        "defaults": {
                            "color": {"mode": "palette-classic"},
                            "custom": {
                                "drawStyle": "line",
                                "fillOpacity": 10,
                                "lineInterpolation": "smooth",
                                "lineWidth": 1,
                                "spanNulls": 86400000,
                            },
                            "unit": "fahrenheit",
                        },
                        "overrides": overrides or [],
                    },
                    "options": {
                        "legend": {
                            "calcs": [],
                            "displayMode": "list",
                            "placement": "right",
                            "showLegend": True,
                        },
                        "tooltip": {"mode": "multi", "sort": "asc"},
                    },
                },
                "version": "13.0.1+security-01",
            },
        },
    }


def bargauge_panel(pid: int, title: str, expr: str, unit: str, min_v: float, max_v: float) -> dict:
    return {
        "kind": "Panel",
        "spec": {
            "data": {
                "kind": "QueryGroup",
                "spec": {
                    "queries": [
                        panel_query(
                            "A",
                            expr,
                            legendFormat="{{thermostat_name}}",
                        )
                    ],
                    "queryOptions": {},
                    "transformations": [],
                },
            },
            "description": "",
            "id": pid,
            "links": [],
            "title": title,
            "vizConfig": {
                "group": "bargauge",
                "kind": "VizConfig",
                "spec": {
                    "fieldConfig": {
                        "defaults": {
                            "color": {"mode": "thresholds"},
                            "min": min_v,
                            "max": max_v,
                            "thresholds": {
                                "mode": "absolute",
                                "steps": [{"color": "green", "value": 0}],
                            },
                            "unit": unit,
                        },
                        "overrides": [],
                    },
                    "options": {
                        "displayMode": "lcd",
                        "orientation": "horizontal",
                        "reduceOptions": {"calcs": ["lastNotNull"], "fields": "", "values": False},
                        "showUnfilled": True,
                    },
                },
                "version": "13.0.1+security-01",
            },
        },
    }


def build() -> dict:
    t = '$ecobee_thermostat_name'
    filt = f'{{thermostat_name=~"{t}"}}'

    panel_2 = {
        "kind": "Panel",
        "spec": {
            "data": {
                "kind": "QueryGroup",
                "spec": {
                    "queries": [
                        panel_query(
                            "A",
                            f'ecobee_currenthvacmode{filt}',
                            instant=True,
                            format="table",
                            legendFormat="{{ thermostat_name }}",
                        ),
                        panel_query(
                            "B",
                            f'ecobee_equipment_running{filt} == 1',
                            instant=True,
                            format="table",
                            legendFormat="{{ equipment }}",
                        ),
                        panel_query(
                            "C",
                            f'ecobee_equipment_runtime_seconds{{interval="2", thermostat_name=~"{t}"}}',
                            instant=True,
                            format="table",
                            legendFormat="{{ equipment }}",
                        ),
                    ],
                    "queryOptions": {},
                    "transformations": [
                        {
                            "group": "filterFieldsByName",
                            "kind": "Transformation",
                            "spec": {
                                "options": {
                                    "include": {
                                        "names": [
                                            "current_hvac_mode",
                                            "thermostat_name",
                                            "equipment",
                                            "Value #C",
                                        ]
                                    }
                                }
                            },
                        },
                        {
                            "group": "joinByField",
                            "kind": "Transformation",
                            "spec": {"options": {"byField": "thermostat_name", "mode": "inner"}},
                        },
                    ],
                },
            },
            "description": "HVAC mode, active equipment, and latest 5-minute runtime bucket (Beehive).",
            "id": 2,
            "links": [],
            "title": "HVAC Status",
            "vizConfig": {
                "group": "table",
                "kind": "VizConfig",
                "spec": {
                    "fieldConfig": {
                        "defaults": {
                            "custom": {"align": "center", "cellOptions": {"type": "color-text"}},
                            "mappings": [
                                {
                                    "type": "value",
                                    "options": {
                                        "cool": {"color": "blue", "index": 1, "text": "Cooling"},
                                        "heat": {"color": "red", "index": 0, "text": "Heating"},
                                        "off": {"color": "super-light-green", "index": 2, "text": "Off"},
                                    },
                                }
                            ],
                            "unit": "string",
                        },
                        "overrides": [],
                    },
                    "options": {"cellHeight": "lg", "showHeader": True},
                },
                "version": "13.0.1+security-01",
            },
        },
    }

    panel_76 = {
        "kind": "Panel",
        "spec": {
            "data": {
                "kind": "QueryGroup",
                "spec": {
                    "queries": [
                        panel_query(
                            "Outside Temp",
                            f'ecobee_forecast_temperature{filt}',
                            instant=True,
                            format="table",
                        ),
                        panel_query(
                            "Dew Point",
                            f'ecobee_forecast_dewpoint{filt}',
                            instant=True,
                            format="table",
                        ),
                        panel_query(
                            "Visibility",
                            f'ecobee_forecast_visibility{filt} * 0.000621371',
                            instant=True,
                            format="table",
                        ),
                        panel_query(
                            "Chance of Rain",
                            f'ecobee_forecast_probability_of_precipitation{filt}',
                            instant=True,
                            format="table",
                        ),
                        panel_query(
                            "High",
                            f'ecobee_forecast_temp_high{filt}',
                            instant=True,
                            format="table",
                        ),
                        panel_query(
                            "Low",
                            f'ecobee_forecast_temp_low{filt}',
                            instant=True,
                            format="table",
                        ),
                    ],
                    "queryOptions": {},
                    "transformations": [
                        {
                            "group": "joinByField",
                            "kind": "Transformation",
                            "spec": {"options": {"byField": "thermostat_name", "mode": "outer"}},
                        },
                        {
                            "group": "limit",
                            "kind": "Transformation",
                            "spec": {"options": {"limitField": 1}},
                        },
                    ],
                },
            },
            "description": "Outdoor weather from Beehive or linked Home Assistant `weather.*` entities.",
            "id": 76,
            "links": [],
            "title": "Outdoor / forecast",
            "vizConfig": {
                "group": "stat",
                "kind": "VizConfig",
                "spec": {
                    "fieldConfig": {
                        "defaults": {"unit": "string"},
                        "overrides": [
                            {
                                "matcher": {"id": "byRegexp", "options": "/Temp|Dew|High|Low/"},
                                "properties": [{"id": "unit", "value": "fahrenheit"}],
                            },
                            {
                                "matcher": {"id": "byName", "options": "Visibility"},
                                "properties": [{"id": "unit", "value": "lengthmi"}],
                            },
                            {
                                "matcher": {"id": "byName", "options": "Chance of Rain"},
                                "properties": [{"id": "unit", "value": "percent"}],
                            },
                        ],
                    },
                    "options": {
                        "colorMode": "background_solid",
                        "graphMode": "none",
                        "textMode": "value_and_name",
                        "wideLayout": True,
                    },
                },
                "version": "13.0.1+security-01",
            },
        },
    }

    outside_line = {
        "matcher": {"id": "byName", "options": "Outside"},
        "properties": [
            {"id": "custom.lineStyle", "value": {"dash": [5, 5], "fill": "dash"}},
            {"id": "color", "value": {"fixedColor": "blue", "mode": "fixed"}},
        ],
    }

    panel_7 = timeseries_panel(
        7,
        "Temperature",
        [
            panel_query(
                "AVG thermostats",
                f'avg(ecobee_actual_temperature{filt})',
                legendFormat="AVG thermostats",
                interval="1h",
            ),
            panel_query(
                "Outside",
                "avg(ecobee_forecast_temperature)",
                legendFormat="Outside",
                interval="1h",
            ),
        ],
        overrides=[outside_line],
        interval="15m",
    )

    panel_29 = timeseries_panel(
        29,
        "Humidity",
        [
            panel_query(
                "AVG thermostats",
                f'avg(ecobee_actual_humidity{filt})',
                legendFormat="AVG thermostats",
                interval="1h",
            ),
            panel_query(
                "Outside",
                "avg(ecobee_forecast_relative_humidity)",
                legendFormat="Outside",
                interval="1h",
            ),
        ],
        overrides=[
            outside_line,
            {
                "matcher": {"id": "byRegexp", "options": "/.*/"},
                "properties": [{"id": "unit", "value": "percent"}],
            },
        ],
        interval="15m",
    )
    panel_29["spec"]["vizConfig"]["spec"]["fieldConfig"]["defaults"]["unit"] = "percent"

    panel_20 = timeseries_panel(
        20,
        "Temperature - $ecobee_thermostat_name",
        [
            panel_query(
                "B",
                f'ecobee_temperature{filt}',
                legendFormat="Temperature: {{sensor_name}}",
            ),
            panel_query(
                "C",
                f'ecobee_target_temperature_min{filt}',
                legendFormat="Set Temp Min {{thermostat_name}}",
            ),
            panel_query(
                "D",
                f'ecobee_target_temperature_max{filt}',
                legendFormat="Set Temp Max {{thermostat_name}}",
            ),
            panel_query(
                "F",
                f'ecobee_humidity{filt}',
                legendFormat="Humidity",
            ),
        ],
        interval="15m",
    )

    return {
        "annotations": [
            {
                "kind": "AnnotationQuery",
                "spec": {
                    "builtIn": True,
                    "enable": True,
                    "hide": True,
                    "name": "Annotations & Alerts",
                    "query": {
                        "datasource": {"name": "-- Grafana --"},
                        "group": "grafana",
                        "kind": "DataQuery",
                        "spec": {},
                        "version": "v0",
                    },
                },
            }
        ],
        "cursorSync": "Off",
        "editable": True,
        "elements": {
            "panel-2": panel_2,
            "panel-7": panel_7,
            "panel-20": panel_20,
            "panel-29": panel_29,
            "panel-63": bargauge_panel(
                63,
                "Current Temperature ($ecobee_thermostat_name)",
                f'ecobee_actual_temperature{filt}',
                "fahrenheit",
                55,
                85,
            ),
            "panel-70": bargauge_panel(
                70,
                "Current Humidity ($ecobee_thermostat_name)",
                f'ecobee_actual_humidity{filt}',
                "percent",
                0,
                100,
            ),
            "panel-76": panel_76,
        },
        "layout": {
            "kind": "RowsLayout",
            "spec": {
                "rows": [
                    {
                        "kind": "RowsLayoutRow",
                        "spec": {
                            "collapse": False,
                            "title": "Current Stats",
                            "layout": {
                                "kind": "GridLayout",
                                "spec": {
                                    "items": [
                                        grid_item("panel-63", 0, 0, 12, 3),
                                        grid_item("panel-2", 12, 0, 12, 6),
                                        grid_item("panel-70", 0, 3, 12, 3),
                                    ]
                                },
                            },
                        },
                    },
                    {
                        "kind": "RowsLayoutRow",
                        "spec": {
                            "collapse": False,
                            "title": "Forecast",
                            "layout": {
                                "kind": "GridLayout",
                                "spec": {"items": [grid_item("panel-76", 0, 0, 24, 6)]},
                            },
                        },
                    },
                    {
                        "kind": "RowsLayoutRow",
                        "spec": {
                            "collapse": False,
                            "title": "Detailed Stats",
                            "layout": {
                                "kind": "GridLayout",
                                "spec": {
                                    "items": [
                                        grid_item("panel-7", 0, 0, 12, 8),
                                        grid_item("panel-29", 12, 0, 12, 8),
                                        {
                                            "kind": "GridLayoutItem",
                                            "spec": {
                                                "element": {
                                                    "kind": "ElementReference",
                                                    "name": "panel-20",
                                                },
                                                "height": 9,
                                                "width": 24,
                                                "x": 0,
                                                "y": 8,
                                                "repeat": {
                                                    "direction": "v",
                                                    "mode": "variable",
                                                    "value": "ecobee_thermostat_name",
                                                },
                                            },
                                        },
                                    ]
                                },
                            },
                        },
                    },
                ]
            },
        },
        "links": [],
        "liveNow": False,
        "preload": False,
        "tags": ["ecobee", "ecobee-exporter"],
        "timeSettings": {
            "autoRefresh": "30m",
            "from": "now-7d",
            "timezone": "browser",
            "to": "now",
        },
        "title": "Ecobee Thermostats",
        "variables": [
            {
                "kind": "QueryVariable",
                "spec": {
                    "allowCustomValue": True,
                    "current": {"text": "All", "value": "$__all"},
                    "definition": "label_values(ecobee_actual_temperature, thermostat_name)",
                    "hide": "hideVariable",
                    "includeAll": True,
                    "label": "Thermostat",
                    "multi": True,
                    "name": "ecobee_thermostat_name",
                    "options": [],
                    "query": {
                        "datasource": DS,
                        "group": "prometheus",
                        "kind": "DataQuery",
                        "spec": {
                            "query": "label_values(ecobee_actual_temperature, thermostat_name)",
                            "refId": "PrometheusVariableQueryEditor-VariableQuery",
                        },
                        "version": "v0",
                    },
                    "refresh": "onTimeRangeChanged",
                    "sort": "alphabeticalAsc",
                },
            }
        ],
    }


def grid_item(name: str, x: int, y: int, width: int, height: int) -> dict:
    return {
        "kind": "GridLayoutItem",
        "spec": {
            "element": {"kind": "ElementReference", "name": name},
            "height": height,
            "width": width,
            "x": x,
            "y": y,
        },
    }


def main() -> None:
    out = Path(__file__).with_name("thermostats.json")
    out.write_text(json.dumps(build(), indent=2) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
