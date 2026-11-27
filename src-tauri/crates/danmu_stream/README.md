## Event format

Stream providers expose each decoded WebSocket event as a
`DanmuMessageType::Event`. Recorders write the events to `events.jsonl`, one
JSON object per line:

```json
{
  "ts": 1710000000123,
  "platform": "bilibili",
  "room_id": "12345",
  "type": "gift",
  "data": {
    "user_id": 42,
    "user_name": "viewer",
    "gift_name": "花束",
    "count": 1,
    "price": 100
  },
  "raw": {
    "cmd": "SEND_GIFT",
    "data": {}
  }
}
```

`ts` is Unix time in milliseconds and `type` is the normalized event
category. Current categories are `danmu`, `gift`, `super_chat`, `enter`,
`like`, `online`, `room_update`, and `unknown`. `data` holds the fields shared
by events in that category. `raw` preserves the provider's original JSON, so
new provider fields remain available without a change to the storage format.
