# FM27: loans

Notes behind showing a player's loan and recalling him from it. Read on the Windows Game Pass build (0.9.2409.0), on
a save of 17 September 2026 whose manager runs Manchester United, with the probe (`refs`, `dump`, `loans`).

## The loan contract

A player on loan keeps his contract with his own club (`player+0x330`) and has a second one with the team he is on
loan to, of its own class, `db::LOAN_CONTRACT` (his own is a `db::FULL_CONTRACT`). It is reached through the pointer
right after his contract:

| Where | What |
| --- | --- |
| player `+0x338` | pointer to a record of his other contracts, shifted with the contract for player-coaches |
| record `+0x00` | pointer to the loan contract, null when he is not on loan |
| record `+0x30` | a `db::BASIC_CONTRACT` of a person who is not a player (likely his agent's) |

Only the player points at the record, and only the record points at the loan contract. The loan contract is laid
out as a contract up to the squad number (`[contract]` in `profiles/fm27/layout.toml`):

| Offset | Field | André Onana, Manchester United → Trabzonspor |
| --- | --- | --- |
| `+0x08` | owner: the player's identity block | Onana |
| `+0x10`, `+0x18` | the team he is on loan to | Trabzonspor |
| `+0x20` | `i32` wage, pounds per week | 113,300 (110,000 in his own contract) |
| `+0x44` | from (the contract's `signed`) | 29 June 2026 |
| `+0x48` | until (`expires`) | 30 June 2027 |
| `+0x4C` | joined | 29 June 2026 |
| `+0x54` | squad status there | 2 |
| `+0x5D` | squad number there | 24, as he wears it at Trabzonspor |

The bytes after the squad number (`+0x60` to `+0x100`) hold the loan's own terms; they are not mapped yet. Onana's
read 3,212 twice at `+0xA0` and `+0xA8`, 58,333 twice at `+0xCC`, his wage again at `+0xD4`, and 180 and 270 at
`+0xD8`.

The record is 32 bytes, laid out one after another with the others of its kind, so reading past its end shows the
next player's:

| Offset | What | Records holding one |
| --- | --- | --- |
| `+0x00` | the loan contract (`db::LOAN_CONTRACT`) | 3,598 |
| `+0x08` | a contract with a club's B team (`db::B_CLUB_CONTRACT`) | 2,189 |
| `+0x10` | a basic or full contract (a few player-coaches) | 11 |
| `+0x18` | another contract with a B team | 60 |

When the game ended the loans of Onana and Bayındır, whose records held nothing else, it left them without a record:
null at `+0x338`. Their team (`+0x130`) pointed at Manchester United's again, while `+0x138` kept Trabzonspor's, the
last team they played for.

## Who has one

On the save, 3,596 players had a loan contract, and every one of them passed the editor's checks (class and owner).
5,637 players had the record, so 2,041 have one without a loan. 5,624 players play for a team of another club than
their contract's: 3,594 of them are on loan; nearly all the rest belong to a club's second team, which the game keeps
as a club of its own (such as "VfB Stuttgart II" or "FC Twente/Heracles Academie").

## The terms

Past the fields it shares with a contract, the loan contract keeps the terms the game's Loan Club tab shows. RootTourist
mapped them on the Steam build 26340721 against 17 loans in the English UI (in a comment on PR #101); the layout places
them as `[loan.terms]`:

| Offset | Type | In the game |
| --- | --- | --- |
| `+0xB0` | `u16` flags | bit 0 "Can play against own team", bit 1 "Can play in cup matches", bit 2 "Can be recalled", bit 12 "Loan cannot be terminated"; bits 3, 8, 13 and 15 occur, and the game shows nothing for them |
| `+0xC0` | `i32` pounds | "Optional Future Fee", the option to buy; -1 for none |
| `+0xCC` | `i32` pounds | "Monthly playing fee"; -1 for none |
| `+0xD0` | `i32` pounds | "Unused Monthly Fee"; -1 for none |
| `+0xD4` | `i32` pounds | the wage again, as at `+0x20` (on 351 of 371 loans) |

On the Steam save of 15 August 2027, 113 of 371 loans could not be recalled in the game and 94 could not be
terminated, 71 neither. Anton Kade's, from FC Augsburg to Liverpool, holds flags `0x1002` (cup matches, no recall, not
to be terminated), an option to buy of £23,245,368 and monthly fees of £371,665 (played) and £725,477 (unused). The
profile lists the terms, the recall dialog says what they would not allow in the game, and Make permanent starts its
fee from the option to buy.

## Recalling a player

The editor never ends a loan itself: the game would have to take him out of one team's list and put him in another,
free the loan contract and tell both clubs. It moves the loan's end (`loan.expires`) instead, and lets the game end
the loan, as at the end of any loan.

**The end is the loan's last day.** In the game, André Onana and Altay Bayındır (Manchester United's goalkeepers on
loan at Trabzonspor and Celta) were recalled with their end moved to the day after the game's date: both came back,
two days on. A loan "until 30 June" is one he plays through 30 June, back at his club on 1 July. So a recall makes the
game's date the loan's last day, and the player is back the next day: one day on.

A day the game has finished would never be one again, and a loan whose last day it was might never end, so the editor
writes only the game's date or a later one. While a loan ends on the game's date, the profile says his return is
pending instead of offering the recall; a loan whose end has passed while he is still away gets a warning and the
recall again.

The game's own recall needs "Can be recalled" (see [The terms](#the-terms)). The editor's does not go through it: it
moves the loan's last day, and the game ends the loan when that day is over.

**A loan that ends may become a transfer.** Anton Kade, recalled by the editor on 15 August 2027 (no recall, not to be
terminated, an option to buy of £23,245,368), was not back at FC Augsburg the next day: Liverpool had signed him for his
option. Its balance and transfer budget were both £23,245,368 lower, and he was on a new contract of £105,519 a week
until 30 June 2033, signed that day. So a recall takes the option to buy away too (`loan.option_fee`, written -1),
which its undo writes back with the loan's end, and the recall dialog says so.

## Making a loan permanent

A player on loan is in a list of the team he plays for, at the club he is on loan to, and in a list of his own club:
its senior team's, or for a youngster the youth team's he played for (Mirza Ćatović, on loan from VfB Stuttgart to
Barcelona, is in Stuttgart's under-19s' list and not in its senior team's). Making the loan permanent (`plan_loan_permanent`)
leaves him where he plays, and writes:

1. his own contract: both team pointers to the senior team of the club he stays at, signed and joined on the game's
   date, the wage and end when given, the squad number and squad status his loan contract has there, and off the
   transfer and loan lists;
2. his own club: a captaincy he holds is left empty, and every list that holds him loses him;
3. the loan, let go as the game lets one go: null at `+0x338` when the record holds nothing else, the loan contract's
   slot in the record otherwise. The loan contract and the record are left with nothing pointing at them.

Each write keeps the bytes it found, so one undo puts the loan back. Checked with the probe's `permanent 2000332528
--restore` (9 writes): while made permanent he read as Barcelona's, not on loan, with the squad status of his loan
contract (13), and afterwards as on loan again from Stuttgart until 30 June 2027. Whether the game takes him as
Barcelona's over the next days is to be checked in the game.

## On the Mac

The Mac Steam build (26340721, arm64) keeps loans the same way, 0x40 earlier in the player as the person's fields
are: his contract at `+0x2F0` and the record of his other contracts right after it, at `+0x2F8`. The record and the
loan contract are laid out as on Windows, and so are the terms (`[loan]` and `[loan.terms]` in `layout-macos.toml`).
On the Arsenal save of 5 July 2027, a few days after most loans had ended:

- 92 players had a loan contract, every one passing the class (`N2db13LOAN_CONTRACTE`) and owner checks; 1,471 had
  a record, whose other slots held 1,370 contracts with B teams (`db::B_CLUB_CONTRACT`) and a few full or basic
  ones, as on Windows. Two records held a trial contract (`db::TRIAL_CONTRACT`) in the loan's slot, which the editor
  leaves alone.
- 1,458 players played for a team of another club than their contract's: the 92 on loan, and nearly all the rest at
  a club's second team.
- The terms read as the Loan Club tab would show them: Maxim De Cuyper (Brighton & Hove Albion to Olympique
  Lyonnais until 29 June 2028) has flags `0x1006`, so he can be recalled but the loan cannot be terminated, an option
  to buy of £32,394,208 and monthly fees of £52,761; Rob Dickie (Bristol City to FC Cincinnati) `0x1002`, neither.

With the probe, each put back: De Cuyper's loan made permanent (`permanent 18110473 --restore`, 11 writes: he read as
Lyon's, not on loan, with his loan contract's squad status, then on loan again), and a recall's writes, his loan's
end to the game's date and his option to buy to -1, each read back and restored.

## Starting a loan

Read on the Mac Steam build 26385295 (Apple silicon), on a Barcelona save of 29 May 2033 with 1,397 players on loan,
with the probe (`loan-holders`, `loans --raw`, `instances db::LOAN_CONTRACT --fields`, `pools`, `refs`).

### What a loan is made of

`loan-holders` finds every pointer into 60 players on loan and 60 players at the same clubs who are not (neither group
on the transfer or loan list, as offers for a player point at him too), and tells each pointer by the record that
holds it: the pool whose slot it is in, or the class of the nearest vtable before it. Only one kind of record pointed at
all 60 players on loan and at none of the others: the loan contract. The others that come with loans come with some of
them only: `LOAN_OFFER` objects (22 of 60), offers for a player of another club; and records of two pools of no class
(27 and 21 of 60, 8 and 3 of the others), most likely what he has played, which the game adds as he plays.

So a loan is what the notes above describe, and nothing more:

1. the loan contract (`db::LOAN_CONTRACT`), from the game's pool of them: 256-byte slots, at `game_plugin+0x574dcb8`
   on this build, with 1,722 used and 3 free on 29 May;
2. the record of the player's other contracts pointing at it, from a pool of 32-byte slots of no class
   (`game_plugin+0x574bbc0`, 1 free). A player with a contract with a B team has one already, its place for a loan
   empty: Roc, of Barcelona B, held his `B_CLUB_CONTRACT` at `+0x08`;
3. the team he plays for: the club's senior team, which lists him; his own club's lists keep him (as Dinho, on loan from
   Barcelona to FC Bayern München, is in both, "loan out" and "loan in").

The pool holds more loan contracts than there are loans: 1,722 against 1,397. The others are held by no record. Dinho,
on Barcelona's loan list, had three more loan contracts of his, with other clubs' teams, beside his loan's: loans
offered for him.

### The loan contract, past what it shares with a contract

What every loan contract holds, across the 1,722 (`instances --fields`) and the 1,397 loans (`loans --raw`):

| Offset | In the loans |
| --- | --- |
| `+0x28`, `+0x30` | null in nearly all; a pointer in 42 and 65 (the reasons he is unhappy, as in a contract) |
| `+0x60` | a small number (0, 1, 0x100, 2) |
| `+0x68` | a pointer to a 200-byte object of the loan's own, in **253**, and only in loans with an option to buy; null in the other 1,144. Each points at its own, and nothing else points at it |
| `+0x70` to `+0x8C` | zero in every loan |
| `+0x90` to `+0x9C` | -1 in every loan |
| `+0xA0`, `+0xA4` | numbers that do not grow with the loan |
| `+0xA8` | 0xC8C (3,212) in every loan |
| `+0xB0` | the flags of the terms (`[loan.terms]`); in 1,029 loans those at `+0xB8` with bit 13 (`0x2000`) set, and `0x0911` in the next two bytes |
| `+0xB8` | the flags as agreed: the terms' bits, without bit 13 |
| `+0xC0` to `+0xD0` | the option to buy and the monthly fees (`[loan.terms]`); 0 for no fee in most loans |
| `+0xD4` | the wage again, in 1,042 loans (0 in 316) |
| `+0xD8`, `+0xDC` | 90, 180, 270 or -1: numbers of days |
| `+0xEC`, `+0xF0` | 1 January 1900, no date, in every loan |
| `+0xF4`, `+0xF9`, `+0xFB` | counts that grow as the loan goes on (Spearman's rho 0.58 to 0.65 against the days since it began), 0 in a loan that has not been played |

### Making one

`Session::plan_loan` (`loans.rs`) and the probe's `loan-start` start a loan as the game holds one (`[loan.start]` in
`layout-macos.toml`; none on Windows yet, where the editor starts no loan):

1. the first free slot of the pool of loan contracts, a copy of the first running loan's in the pool with no option to
   buy whose flags are as most loans' are: the player's, with the club's senior team, from today to the loan's last day
   (by default 30 June, or his contract's end when that comes first, and never past it), on the wage given or his own
   (at `+0x20` and `+0xD4`), with the squad status of his own contract and how happy he is with it, no squad number,
   off the lists. The terms given (recall, termination, cup matches, against his own club) at `+0xB8`, and at `+0xB0`
   with bit 13; no option to buy, no fees. `+0x28`, `+0x30`, `+0x68` and the counts at `+0xF4` to `+0xFB` cleared;
2. his record pointing at it: his own, where its place for a loan is empty (a trial contract there is left alone, and
   the loan refused), or the first free slot of the pool of records;
3. the club's senior team's list holding him, grown when it is full;
4. his team: that senior team. His own contract, and his club's lists, stay as they are; a captaincy he holds at the
   club he played for is left empty, and a club a human manager runs leaves him out of its selections.

One undo takes it all back, both slots first in their pools' chains again.

### Checked

With the probe, on the Barcelona save of 29 May 2033:

- `loan-start 2002109237 814089 --restore --hold 180`: Roc (19, Barcelona B, under contract with Barcelona until 30
  June 2033) on loan to Girona until 30 June, 6 writes. The game's profile said "On loan at Girona", "Parent Club
  Barcelona" and "Out On Loan", with a Loan Report and a 32-33 row for Girona in his career. Put back, the loan
  contract's slot was free again (3 free, 1,722 used), his record's place for a loan empty, he was out of Girona's
  list and at Barcelona B; the game showed no loan and no row for Girona.
- `loan-start 2002109237 814089 --until 2033-06-02`, played on to 4 June: the game ended the loan itself on its last
  day. Roc was back at Barcelona B, out of Girona's list, his record's place for a loan cleared (his B team contract
  kept), and the loan contract's slot free.

### On Windows

Read only, on Steam build 26385295 on Windows, on a Barcelona save of 14 June 2033 with 1,419 loans (`loans --raw`,
`pools --holding`, `loan-holders`). What `[loan.start]` relies on is as on the Mac:

- the loan contracts take 256-byte slots of the game's pool of them (`game_plugin.dll+0x581d198`, 1,696 used), and
  the records of a player's other contracts 32-byte slots of theirs (`+0x58265b0`, 5,745 used): Pape Matar Sarr's
  record, at his `+0x338`, is slot 391 of that pool, and its `+0x00` points at his loan contract;
- in 1,125 loans the flags at `+0xB0` (as a `u16`) are those at `+0xB8` with bit 13 set, and in 1,347 `+0xD4` holds
  the wage again; of the 71 loans that began in June, 67 read 0 in the counts at `+0xF4` and `+0xF8`;
- `+0x70` to `+0x8F` are zero and `+0xA8` is 0xC8C in every loan. `+0x90` is -1 in all but 2, and `+0x9C` in all but
  8: none of them places a loan started by the editor writes.

Both pools had no free slot on that day, so `loan-start` would have been refused: it takes only a free slot. No loan
has been started on Windows yet, so `layout.toml` has no `[loan.start]`.
