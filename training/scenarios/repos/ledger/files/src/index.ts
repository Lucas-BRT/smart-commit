#!/usr/bin/env node
import { readFileSync } from "node:fs";

import { Registry } from "./accounts.js";
import { Money } from "./money.js";
import { balances } from "./report.js";
import { Journal } from "./transactions.js";

interface Book {
  accounts: { name: string; kind: "asset" | "liability" | "income" | "expense" }[];
  transfers: { from: string; to: string; amount: string; memo: string; date: string }[];
}

function load(path: string): { registry: Registry; journal: Journal } {
  const book: Book = JSON.parse(readFileSync(path, "utf8"));
  const registry = new Registry();
  for (const account of book.accounts) {
    registry.open(account.name, account.kind);
  }
  const journal = new Journal(registry);
  for (const t of book.transfers) {
    journal.record({ ...t, amount: Money.parse(t.amount) });
  }
  return { registry, journal };
}

const [command, path = "book.json"] = process.argv.slice(2);
if (command === "balance") {
  console.log(balances(load(path).registry));
} else {
  console.error("usage: ledger balance [book.json]");
  process.exit(2);
}
