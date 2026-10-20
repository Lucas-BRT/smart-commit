import assert from "node:assert/strict";
import { test } from "node:test";

import { Registry } from "../src/accounts.js";
import { Money } from "../src/money.js";
import { Journal } from "../src/transactions.js";

test("a transfer moves money between balances", () => {
  const registry = new Registry();
  registry.open("cash", "asset");
  registry.open("food", "expense");
  const journal = new Journal(registry);
  journal.record({ from: "cash", to: "food", amount: Money.parse("9.90"), memo: "lunch", date: "2026-01-02" });
  assert.equal(registry.get("cash").balance.toString(), "-9.90");
  assert.equal(registry.get("food").balance.toString(), "9.90");
});
