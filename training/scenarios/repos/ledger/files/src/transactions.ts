import { Registry } from "./accounts.js";
import { Money } from "./money.js";

export interface Transfer {
  from: string;
  to: string;
  amount: Money;
  memo: string;
  date: string;
}

export class Journal {
  readonly entries: Transfer[] = [];

  constructor(private registry: Registry) {}

  record(transfer: Transfer): void {
    if (transfer.amount.isNegative()) {
      throw new Error("transfers move a positive amount");
    }
    const from = this.registry.get(transfer.from);
    const to = this.registry.get(transfer.to);
    from.balance = from.balance.minus(transfer.amount);
    to.balance = to.balance.plus(transfer.amount);
    this.entries.push(transfer);
  }

  between(start: string, end: string): Transfer[] {
    return this.entries.filter((t) => t.date >= start && t.date <= end);
  }
}
