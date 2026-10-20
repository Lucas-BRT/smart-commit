import assert from "node:assert/strict";
import { test } from "node:test";

import { Money } from "../src/money.js";

test("parses whole and fractional amounts", () => {
  assert.equal(Money.parse("12").cents, 1200);
  assert.equal(Money.parse("12.5").cents, 1250);
  assert.equal(Money.parse("-0.07").cents, -7);
});

test("prints two decimals", () => {
  assert.equal(Money.fromCents(1205).toString(), "12.05");
  assert.equal(Money.fromCents(-5).toString(), "-0.05");
});
