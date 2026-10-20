import { Account, Registry } from "./accounts.js";

function line(account: Account, width: number): string {
  return `${account.name.padEnd(width)}  ${account.balance.toString().padStart(12)}`;
}

/** One line per acount, with its balance. */
export function balances(registry: Registry): string {
  const accounts = registry.all();
  const width = Math.max(7, ...accounts.map((a) => a.name.length));
  return [`${"account".padEnd(width)}  ${"balance".padStart(12)}`, ...accounts.map((a) => line(a, width))].join("\n");
}
