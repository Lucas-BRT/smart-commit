# Usage

Keep the book in a JSON file:

```json
{
  "accounts": [{ "name": "cash", "kind": "asset" }, { "name": "food", "kind": "expense" }],
  "transfers": [{ "from": "cash", "to": "food", "amount": "9.90", "memo": "lunch", "date": "2026-01-02" }]
}
```

Then print the balances:

```bash
ledger balance book.json
```
