import { Money } from "./money.js";

export type AccountKind = "asset" | "liability" | "income" | "expense";

export interface Account {
  name: string;
  kind: AccountKind;
  balance: Money;
}

export class Registry {
  private accounts = new Map<string, Account>();

  open(name: string, kind: AccountKind): Account {
    if (this.accounts.has(name)) {
      throw new Error(`account already exists: ${name}`);
    }
    const account = { name, kind, balance: Money.fromCents(0) };
    this.accounts.set(name, account);
    return account;
  }

  get(name: string): Account {
    const account = this.accounts.get(name);
    if (!account) {
      throw new Error(`no such account: ${name}`);
    }
    return account;
  }

  all(): Account[] {
    return [...this.accounts.values()];
  }
}
