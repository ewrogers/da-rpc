# Effects

Effects are the small timed spell icons shown by the client. Players often
recognize their remaining duration by the color of the bar rather than an exact
number of seconds. daRPC exposes the same relative stages.

| Use | Route or events |
| --- | --- |
| Read active effects | `GET /clients/{client}/effects` |
| Watch effect changes | [Persistent effect events](events.md#persistent-effect-events) |

## Reading effects

```text
GET /clients/{client}/effects
```

Each active effect contains:

- `icon`, which identifies the displayed effect
- `duration`, which is a relative remaining-time band

```text
Effects {
    observation: ObservationMetadata,
    effects: Effect[]?,
}

Effect {
    icon: u16,
    duration: EffectDuration,
}
```

From longest to shortest, the duration values are:

```text
white, red, orange, yellow, green, blue
```

These values are not exact timers. The client retains the visible phase, so
daRPC does not invent a remaining number of seconds.

The initial baseline reads the client's ten effect slots. Later effect updates
keep the retained resource current without another complete memory capture.

## Effect events

The complete payload structures are in
[Persistent effect events](events.md#persistent-effect-events).

| Event | Meaning | Data |
| --- | --- | --- |
| `effect.added` | A new icon became active. | `icon`, `duration` |
| `effect.changed` | An active icon moved to another duration band. | `icon`, new `duration` |
| `effect.removed` | The icon expired or was cleared. | `icon` |

Effects are identified by icon. A removed event has no duration because the
effect is no longer active.

## Availability

`effects: null` means the effect slots were unavailable. An empty array means
the slots were read successfully and no effects were active.
