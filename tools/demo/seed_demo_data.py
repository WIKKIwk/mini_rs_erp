#!/usr/bin/env python3
"""Seed demo catalog data through the public mini ERP API.

The script is intentionally API-based instead of direct SQL so it follows the
same stores, validation, generated refs, and assignment logic used by mobile.
It is safe to rerun: demo items and groups are upserted, existing demo users are
reused by phone number, and assignments are re-applied idempotently.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any


BASE_URL = os.environ.get("MINI_ERP_DEMO_BASE_URL", "http://127.0.0.1:18081").rstrip("/")
ADMIN_PHONE = os.environ.get("MINI_ERP_DEMO_ADMIN_PHONE", "+998880000000")
ADMIN_CODE = os.environ.get("MINI_ERP_DEMO_ADMIN_CODE", "19621978")


@dataclass(frozen=True)
class DemoItem:
    code: str
    name: str
    uom: str
    item_group: str
    assign_to_customer: bool = False
    assign_to_supplier: bool = False


CUSTOMER = {
    "name": "Demo Customer Textile",
    "phone": "+998881230001",
}

SUPPLIER = {
    "name": "Demo Raw Material Supplier",
    "phone": "+998881230002",
}

ITEM_GROUPS = [
    {"name": "Demo Catalog", "parent": "All Item Groups", "is_group": True},
    {"name": "Demo Raw Materials", "parent": "Demo Catalog", "is_group": True},
    {"name": "Demo Finished Goods", "parent": "Demo Catalog", "is_group": True},
]

ITEMS = [
    DemoItem(
        code="DEMO-FG-BAG-001",
        name="Demo Printed Package 250g",
        uom="pcs",
        item_group="Demo Finished Goods",
        assign_to_customer=True,
    ),
    DemoItem(
        code="DEMO-FG-BAG-002",
        name="Demo Printed Package 500g",
        uom="pcs",
        item_group="Demo Finished Goods",
        assign_to_customer=True,
    ),
    DemoItem(
        code="DEMO-RM-FILM-001",
        name="Demo BOPP Film Roll",
        uom="kg",
        item_group="Demo Raw Materials",
        assign_to_supplier=True,
    ),
    DemoItem(
        code="DEMO-RM-INK-001",
        name="Demo Printing Ink",
        uom="kg",
        item_group="Demo Raw Materials",
        assign_to_supplier=True,
    ),
    DemoItem(
        code="DEMO-RM-GLUE-001",
        name="Demo Lamination Glue",
        uom="kg",
        item_group="Demo Raw Materials",
        assign_to_supplier=True,
    ),
]

WAREHOUSES = [
    {
        "warehouse": "Demo Raw Material Warehouse",
        "company": "Demo",
        "is_group": False,
        "parent_warehouse": "",
    },
    {
        "warehouse": "Demo Finished Goods Warehouse",
        "company": "Demo",
        "is_group": False,
        "parent_warehouse": "",
    },
]


class ApiError(RuntimeError):
    pass


def api(path: str, method: str = "GET", payload: dict[str, Any] | None = None, token: str = ""):
    body = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        body = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"

    request = urllib.request.Request(
        f"{BASE_URL}{path}",
        data=body,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            raw = response.read()
            return json.loads(raw.decode("utf-8")) if raw else None
    except urllib.error.HTTPError as error:
        details = error.read().decode("utf-8", errors="replace")
        raise ApiError(f"{method} {path} failed: HTTP {error.code} {details}") from error
    except urllib.error.URLError as error:
        raise ApiError(f"{method} {path} failed: {error.reason}") from error


def quote(value: str) -> str:
    return urllib.parse.quote(value, safe="")


def normalize_phone(value: str) -> str:
    return "".join(ch for ch in value if ch.isdigit())


def login() -> str:
    response = api(
        "/v1/mobile/auth/login",
        "POST",
        {"phone": ADMIN_PHONE, "code": ADMIN_CODE},
    )
    token = response.get("token") if isinstance(response, dict) else ""
    if not token:
        raise ApiError("admin login did not return a token")
    return token


def paged(path: str, token: str, limit: int = 50) -> list[dict[str, Any]]:
    items: list[dict[str, Any]] = []
    offset = 0
    while True:
        separator = "&" if "?" in path else "?"
        page = api(f"{path}{separator}limit={limit}&offset={offset}", token=token)
        if not isinstance(page, list) or not page:
            break
        items.extend(page)
        if len(page) < limit:
            break
        offset += limit
    return items


def find_by_phone(path: str, phone: str, token: str) -> dict[str, Any] | None:
    target = normalize_phone(phone)
    for entry in paged(path, token):
        if normalize_phone(str(entry.get("phone", ""))) == target:
            return entry
    return None


def ensure_customer(token: str) -> dict[str, Any]:
    existing = find_by_phone("/v1/mobile/admin/customers/list", CUSTOMER["phone"], token)
    if existing:
        return existing
    return api("/v1/mobile/admin/customers", "POST", CUSTOMER, token)


def ensure_supplier(token: str) -> dict[str, Any]:
    existing = find_by_phone("/v1/mobile/admin/suppliers/list", SUPPLIER["phone"], token)
    if existing:
        return existing
    return api("/v1/mobile/admin/suppliers", "POST", SUPPLIER, token)


def ref_of(entry: dict[str, Any]) -> str:
    ref = str(entry.get("ref", "")).strip()
    if not ref:
        raise ApiError(f"missing ref in response: {entry}")
    return ref


def ensure_item_groups(token: str) -> None:
    for group in ITEM_GROUPS:
        api("/v1/mobile/admin/item-groups", "POST", group, token)


def ensure_items(customer_ref: str, supplier_ref: str, token: str) -> None:
    for item in ITEMS:
        payload = {
            "code": item.code,
            "name": item.name,
            "uom": item.uom,
            "item_group": item.item_group,
            "customer_ref": "",
        }
        api("/v1/mobile/admin/items", "POST", payload, token)
        if item.assign_to_customer:
            api(
                f"/v1/mobile/admin/customers/items/add?ref={quote(customer_ref)}",
                "POST",
                {"item_code": item.code},
                token,
            )
        if item.assign_to_supplier:
            api(
                f"/v1/mobile/admin/suppliers/items/add?ref={quote(supplier_ref)}",
                "POST",
                {"item_code": item.code},
                token,
            )


def ensure_warehouses(customer_ref: str, supplier_ref: str, token: str) -> None:
    for warehouse in WAREHOUSES:
        api("/v1/mobile/admin/warehouses", "POST", warehouse, token)

    api(
        "/v1/mobile/admin/warehouses/assignments",
        "POST",
        {
            "warehouse": "Demo Raw Material Warehouse",
            "principal_role": "supplier",
            "principal_ref": supplier_ref,
            "display_name": SUPPLIER["name"],
        },
        token,
    )
    api(
        "/v1/mobile/admin/warehouses/assignments",
        "POST",
        {
            "warehouse": "Demo Finished Goods Warehouse",
            "principal_role": "customer",
            "principal_ref": customer_ref,
            "display_name": CUSTOMER["name"],
        },
        token,
    )


def detail(path: str, ref: str, token: str) -> dict[str, Any]:
    return api(f"{path}?ref={quote(ref)}", token=token)


def ensure_customer_code(customer_ref: str, token: str) -> dict[str, Any]:
    customer_detail = detail("/v1/mobile/admin/customers/detail", customer_ref, token)
    if str(customer_detail.get("code", "")).strip():
        return customer_detail
    return api(
        f"/v1/mobile/admin/customers/code/regenerate?ref={quote(customer_ref)}",
        "POST",
        token=token,
    )


def main() -> int:
    token = login()
    ensure_item_groups(token)
    customer = ensure_customer(token)
    supplier = ensure_supplier(token)
    customer_ref = ref_of(customer)
    supplier_ref = ref_of(supplier)
    ensure_items(customer_ref, supplier_ref, token)
    ensure_warehouses(customer_ref, supplier_ref, token)

    customer_detail = ensure_customer_code(customer_ref, token)
    supplier_detail = detail("/v1/mobile/admin/suppliers/detail", supplier_ref, token)

    summary = {
        "base_url": BASE_URL,
        "customer": {
            "ref": customer_ref,
            "name": customer_detail.get("name"),
            "phone": customer_detail.get("phone"),
            "code": customer_detail.get("code"),
            "assigned_items": [item.get("code") for item in customer_detail.get("assigned_items", [])],
        },
        "supplier": {
            "ref": supplier_ref,
            "name": supplier_detail.get("name"),
            "phone": supplier_detail.get("phone"),
            "code": supplier_detail.get("code"),
            "assigned_items": [item.get("code") for item in supplier_detail.get("assigned_items", [])],
        },
        "warehouses": [warehouse["warehouse"] for warehouse in WAREHOUSES],
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ApiError as error:
        print(f"seed demo failed: {error}", file=sys.stderr)
        raise SystemExit(1)
