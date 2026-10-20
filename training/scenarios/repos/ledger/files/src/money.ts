/** An amount of money in integer cents, so sums never drift. */
export class Money {
  private constructor(readonly cents: number) {}

  static fromCents(cents: number): Money {
    if (!Number.isInteger(cents)) {
      throw new Error(`not a whole number of cents: ${cents}`);
    }
    return new Money(cents);
  }

  static parse(text: string): Money {
    const match = /^(-?)(\d+)(?:\.(\d{1,2}))?$/.exec(text.trim());
    if (!match) {
      throw new Error(`not an amount: ${text}`);
    }
    const [, sign, units, fraction = "0"] = match;
    const cents = Number(units) * 100 + Number(fraction.padEnd(2, "0"));
    return new Money(sign ? -cents : cents);
  }

  plus(other: Money): Money {
    return new Money(this.cents + other.cents);
  }

  minus(other: Money): Money {
    return new Money(this.cents - other.cents);
  }

  isNegative(): boolean {
    return this.cents < 0;
  }

  toString(): string {
    const sign = this.cents < 0 ? "-" : "";
    const abs = Math.abs(this.cents);
    return `${sign}${Math.floor(abs / 100)}.${String(abs % 100).padStart(2, "0")}`;
  }
}
