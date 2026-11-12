# FM27: the share of transfer revenue made available

Notes behind the club field `finance.transfer_revenue`: the share of each player sale's fee that the board adds to
the transfer budget, shown on a club's Finances screen as the "percentage of transfer revenue made available".
Found on the Mac Steam build 26385295 (Apple silicon), on a Barcelona save, on 10 November 2026.

## Where it is

| Record | Offset | What it holds |
| --- | --- | --- |
| The club's full finance record (`[club_finance]`, the class `game_plugin+0x5639f18` on that build) | `+0x7D0` | `i32`, 0 to 100: the percentage, as the screen shows it |

It sits 12 bytes after the transfer budget (`+0x7C4`), among the budgets the board sets. Like them it is only in the
full finance record, which only clubs in the leagues being played have.

## Finding it

`fmlibre-probe finance-find 1708 60`, with Barcelona's Finances screen showing 60%, read the first `0x1000` bytes
of Barcelona's finance record. It looked for 60 as a `u8`, `u16`, `i32` or `f32`, and for 0.6 as an `f32`. It
compared each place with the same place in the records of the eight other clubs with the highest reputation and a
full record: Blu-neri Milano, Arsenal, Paris Saint-Germain, Chelsea, Chelsea (women), Real Madrid, Manchester City
and Juventus, in that order:

| Offset | Barcelona | The other clubs | |
| --- | --- | --- | --- |
| `+0x38` | 60 | 60 60 60 60 0 60 60 60 | the same nearly everywhere: a constant of the game's, not the board's choice |
| `+0x7D0` | 60 | 100 100 100 25 100 100 25 100 | **the percentage** |
| `+0x8E8` | 60 | 0 60 60 0 0 60 60 60 | only 0 or 60 |
| `+0xD08` (`u8`) | 60 | 0 80 164 40 0 140 132 246 | not a percentage for 4 of 8 |
| `+0xDF9` (`u8`) | 60 | 213 149 0 0 110 42 188 155 | not a percentage for 5 of 8 |

Only `+0x7D0` varies from club to club the way a board's choice would. It is a whole `i32`: no club's value carries
over into the bytes after it.

## Confirming it

`fmlibre-probe finance-try 1708 0x7d0 45 --restore --hold 180` wrote 45 there, with the checks of every write
(Barcelona's full finance record, 0 to 100 there now, read back as written). During the hold, Barcelona's Finances
screen showed 45%. The probe then put 60 back.

## On Windows

The same place, on Steam build 26385295 on Windows, on a Barcelona save of 14 June 2033 (the full finance record's class
`game_plugin.dll+0x4df4e38`). Barcelona's Finances screen showed 100%, and `+0x7C4` held the transfer budget the `club`
command reads (£384,681,863). `fmlibre-probe finance-find 1708 100` found two places:

| Offset | Barcelona | The other clubs | |
| --- | --- | --- | --- |
| `+0x688` (`u8`) | 100 | 120 83 212 185 212 148 92 180 | not a percentage for 6 of 8 |
| `+0x7D0` | 100 | 100 100 100 25 100 100 25 100 | **the percentage**, the same values as on the Mac |

`fmlibre-probe finance-try 1708 0x7d0 45 --restore --hold 180` wrote 45 there (read back as written). During the hold,
Barcelona's Finances screen showed 45%. The probe then put 100 back.

Barcelona held 60 on the Mac save of 29 May 2033 and 100 on this one of 14 June 2033, while the other eight clubs held
the same values on both. Whether that is the board setting it again at the end of the season is not known: see Open.

## In the editor

- `transfer_revenue = 0x7D0` in `[club_finance]` of `profiles/fm27/layout.toml` and `profiles/fm27/layout-macos.toml`.
- The field `finance.transfer_revenue`: 0 to 100, kept as a `u32`, which writes the same four bytes as the game's
  `i32` for those values and keeps the app from treating it as money. A club whose value is not 0 to 100 has it
  left alone. Its changes are undone like every field's.
- The app shows it, as a percentage, in a club's Finances, and the probe's `club` command lists it.

## Open

- **What the game does with it.** Whether a sale after a change adds the new share of the fee to the transfer
  budget, and whether the board sets the percentage again at the start of a season or after a board request, has
  not been tried yet.
