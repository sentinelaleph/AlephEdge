# DCA and Grid bots guide

This guide is for someone who has never used a DCA or Grid bot. You do not need to read it all: the first two sections are enough to start, the rest is for when you need it.

> DCA and Grid bots run **simulated (paper)** by default: no order reaches the exchange and you cannot lose money. Real money exists only in the live build, for a Binance Futures bot, after you type **LIVE** for that bot. Details: [Real money](#/guide?s=real-money).

## Start in 5 minutes {#start}

1. Open [Presets](#/presets) in the sidebar.
2. Pick a template whose "Verdict" column says **Passed checks** and press **Use preset**.
3. Pick a large, liquid coin as the **Pair** (BTCUSDT, ETHUSDT).
4. Enter a **Budget**. It is simulated, so no real money is needed.
5. Check that the **Summary** panel on the right says "Budget check: Fits" and that the budget is at least the **Minimum budget** row.
6. Press **Create and start**.

**Budget on a fresh install:** the simulated balance is 1000 USDT at the Cautious level, so all DCA and Grid bots together may use at most 200 USDT. DCA Long Classic needs at least 176.98 USDT and fits. DCA Long Safe needs at least 218.10 USDT and does not fit until you raise the balance or the level on [Risk & safety](#/risk). The create page lowers the opening budget to what is free under the cap when that still clears the template's minimum; otherwise a banner names the minimum and links to Risk & safety.

The bot now trades on real 1-minute prices, fees included, in simulation. Follow it on the bot page under **Cycles**.

Running a template for a few days before touching any setting is the fastest way to learn.

## Core concepts {#concepts}

Each concept below is a setting in the form.

| Concept | Meaning | Why it matters |
|---|---|---|
| **Spot** | You actually buy the coin | No leverage, no liquidation; you only gain when price rises |
| **Futures** | You trade a contract on the coin's price | You can gain from rises (long) and falls (short); leverage is possible |
| **Long** | You gain if price rises | A long DCA buys in steps as price falls |
| **Short** | You gain if price falls | A short DCA sells in steps as price rises |
| **Leverage** | A position larger than your budget | At 5x a 1% price move is 5% of your budget, gains and losses alike |
| **Margin** | Collateral set aside for the position | With isolated margin only this bot's budget is at risk |
| **Liquidation** | The exchange force-closes a position whose loss used up its collateral | The biggest risk of a leveraged bot with no stop: the whole cycle budget is gone |
| **Maker fee** | Fee for a resting limit order that fills (0.02%) | Safety orders and take profit are limit orders: cheap |
| **Taker fee** | Fee for an immediate market order (0.05%) | Base order, stop loss and trailing exit are market orders |
| **Slippage** | A market order filling slightly worse than expected (0.02%) | The simulation adds it to every market order |
| **Funding** | A fee paid between longs and shorts every 8 hours on futures | Raises the cost of a position held for a long time |

A **1x long futures** position is very close to buying spot: no liquidation, only the funding difference.

## How DCA works {#dca}

DCA (Dollar Cost Averaging) rests on one idea: if you buy a little more as price falls, your average buy price falls too. Even a small bounce then puts you in profit.

A DCA bot works in **cycles**:

1. The **base order** buys at market right away.
2. As price falls, **safety orders** fill at preset levels and the average entry falls.
3. When price rises the **take profit** percentage above the average entry, the whole position is sold.
4. The cycle closes and the bot starts a new one at once.

### A worked example

The `dca_long_classic` template, 1,000 USDT budget, 1x, coin price 100. 8 safety orders; first deviation 2.5%, step scale 1.3, volume scale 1.4, take profit 2%.

| Order | Drop from start | Price | Order size | Total invested | Average entry | Sell price (take profit) |
|---|---|---|---|---|---|---|
| Base | 0% | 100.00 | 28.3 | 28.3 | 100.00 | 102.00 |
| Safety 1 | 2.5% | 97.50 | 28.3 | 56.5 | 98.73 | 100.71 |
| Safety 2 | 5.75% | 94.25 | 39.6 | 96.1 | 96.84 | 98.77 |
| Safety 3 | 9.98% | 90.03 | 55.4 | 151.4 | 94.23 | 96.11 |
| Safety 4 | 15.47% | 84.53 | 77.5 | 229.0 | 90.71 | 92.52 |
| Safety 5 | 22.61% | 77.39 | 108.5 | 337.5 | 85.95 | 87.67 |
| Safety 6 | 31.89% | 68.11 | 152.0 | 489.4 | 79.49 | 81.08 |
| Safety 7 | 43.96% | 56.04 | 212.7 | 702.2 | 70.55 | 71.96 |
| Safety 8 | 59.64% | 40.36 | 297.8 | 1,000.0 | 57.69 | 58.85 |

Four things follow from this table:

- **Small drops close fast.** If price falls to 94.25 and comes back to 98.77, the cycle closes in profit, still below the starting price.
- **Most of the budget waits deep down.** The first three orders use only 15% of the budget; the last two use 51%.
- **The ladder covers a 59.6% drop.** Past that the bot cannot buy more; it holds the position and waits for a recovery.
- **Profit is small but frequent.** Each cycle earns about 2% of what it invested. The return comes from many short cycles.

### The risk in DCA

DCA has one big risk: **price falling for a long time without coming back.** With no stop the bot keeps holding: the budget stays locked and an unrealised loss shows. In our tests the longest cycle stayed open 329 days.

Leverage turns that risk into liquidation. A 1x long cannot be liquidated; a 5x long is liquidated about 20% below its average entry. A short can be liquidated even at 1x, when price roughly doubles. That is why the app asks you to confirm ("Liquidation possible, no stop") whenever the Summary shows a liquidation price and no stop sits before it: a DCA with the stop loss off, or a grid with **Stop beyond range** off.

## How Grid works {#grid}

A Grid bot splits the price range you set into equal steps. It places a buy on every step below the start price and a sell on every step above it.

- When a buy fills, a sell goes one step higher.
- When a sell fills, a buy goes one step lower.
- Each time price crosses a step down and back up, one step of profit is taken.

### A worked example

Price 100, range 10% below and 10% above (90 to 110), 20 geometric intervals.

- Each step is about **1.008%** wide.
- Buy and sell are maker orders (0.02% each). After fees each step earns about **0.97%**.
- As price moves back and forth inside the range, these small profits add up.

### The risk in Grid

A Grid loses when **price leaves the range**:

- **Below the range**, every buy has filled and the sells wait: you hold coins worth less.
- **Above the range**, everything is sold: you miss the rest of the rise.

**Stop beyond range** closes the grid once price trades a set percentage beyond the range and caps the loss. In our tests the grid designs did poorly overall: long sideways stretches are rare in real markets, trends are common.

## Which bot for which market {#which-bot}

Nobody knows the market's direction for sure. This table is a starting point, not advice. Each template's real test result is in the "Verdict" column on [Presets](#/presets).

| Market | Bot | Why | Watch out |
|---|---|---|---|
| Rising long term, frequent pullbacks | DCA long classic | Buys cheaper in pullbacks, sells on the bounce | Passed the test; the budget locks in a long bear market |
| Uncertain, large coins | DCA long, safe template | A deep ladder lasts through long drops | Passed the test; profit comes slower |
| Downtrend | DCA short | Sells in steps on rallies, closes on the drop | Both short templates were liquidated at 1x in the test |
| Sideways, narrow band | Grid neutral | Earns from price moving back and forth | Every grid template lost money in the test |
| Slowly rising | Grid long with trailing up | The range follows price upward | Did not pass the test |
| Very volatile altcoin | DCA long, altcoin template, small budget | A wide ladder lasts through sharp moves | Did not pass the test; the coin may not recover |

**General rules:**

- Start with large, liquid coins. Templates tested on the top 5 coins do not behave the same on small coins.
- Start at 1x. Leverage enlarges risk, not profit.
- Do not run several bots on one coin. The app allows one bot per coin, market and side.

## Templates and their test results {#templates}

Every template on [Presets](#/presets) was tested on 2 years of Binance futures data with the same simulator. Fees, slippage and the worst order of moves inside each candle are included. Every figure is per bot-month: a new 1000 USDT bot per pair each month, with a cycle still open at month end left to run on until it closes. It is not one bot kept running for months. Results are split into three periods:

- **Training (TRAIN):** October 2024 to June 2025.
- **Validation (VALID):** July 2025 to January 2026.
- **Test (TEST):** February 2026 to September 2026. The settings were chosen without looking at this period.

A template is marked **Passed checks** only if:

1. The average return per bot-month is positive in all three periods.
2. In the test period the lower end of the 95% confidence interval is above zero.
3. All 8 test months are positive.
4. Nothing was liquidated.

A template that misses even one is marked **Did not pass**, with the checks it missed. These templates stay in the list for learning and trying; the app asks before you use one.

**From a signal:** **Execute** on the [Signals](#/signals) page opens the signal now as a paper position on a fitting signal bot (at the current price when the signal's entry has passed, while the price is between the stop and the target; its filters skipped, its risk limits kept, never real money), or lists the templates that fit the signal's side and opens one on the signal's pair. On that pair the template is untested.

### Result of the 3 October 2026 test

**2 of 12** templates passed all four checks:

| Template | Test period, per bot-month | Positive test months | Worst bot drawdown |
|---|---|---|---|
| DCA Long Classic | +1.90% | 8/8 | −20.7% |
| DCA Long Safe | +1.45% | 8/8 | −15.8% |

**Re-run on 9 October 2026.** On the same test window (to 27 September) both still pass; the figures above are from that re-run. With October added, both miss one check, "every test month positive": in October's first 8.5 days, with the majors down 1 to 15% and every deal still open, Classic was at −0.44% and Safe at −0.14% per bot-month. They are marked **Under review** and the checks are read again on 1 November, when October is complete. No other template passed, and none of the six new candidates tested that day became a template.

What the others teach:

- **DCA Long Quick** did very well in the test period (+5.63% per bot-month) but lost 6.84% per bot-month in validation. A short ladder spends the budget early in a sharp drop.
- **DCA Long BTC** made money in two periods and lost slightly in validation.
- **DCA Long Altcoins** lost money in the test; its worst bot lost 97% of its budget.
- **DCA Long with Stop** sold at the bottom because of the stop and missed the recovery; it lost in validation.
- Both **DCA Short** templates were **liquidated even at 1x**. A 1x short is liquidated when price roughly doubles, and crypto can do that within months.
- All four **Grid** templates lost money in most periods. When price left the range they closed at a loss by stop or time limit.

### The columns

| Column | Meaning |
|---|---|
| Test per bot-month | Average return per bot-month in the test period, % of budget |
| 95% interval | The confidence interval of that average. The narrower and further from zero, the more reliable |
| Months positive | Profitable test months / all test months |
| Drawdown | The deepest drawdown of the worst bot in the test period |
| Verdict | Passed checks or Did not pass |
| Ghost since 8 Oct | The template run forward on the server since 8 Oct 2026, simulated, no money involved. Total return since the start, not per month. Not ranked before 30 days |

**Using a template keeps its link only while your bot matches it.** Name and budget are free; any other change drops the link, and the form says so. The banner names the pairs the template was tested on. BTC-only templates need BTCUSDT. For templates tested on a ranked list (the top 5 by volume, or the altcoin template's pairs ranked 6 to 15), the banner says your pair is not checked against that list.

> A historical simulation does not guarantee future returns. The data holds no long 2022-style bear market. A passed template only shows it worked in these 2 years under these rules.

## DCA settings one by one {#dca-settings}

### Bot section

| Setting | What it does | Suggestion |
|---|---|---|
| Name | How you recognise the bot in lists | Write the coin and the strategy: "BTC DCA safe" |
| Market | Spot or Futures | Futures 1x is very close to spot and allows short; a 1x short is liquidated when price roughly doubles |
| Direction | Long or Short | Long if unsure |
| Pair | Which coin | Large, liquid coins |
| Budget | The most this bot may use | A small part of your total balance |
| Leverage | Position multiplier | 1x |

### Orders and exits

| Setting | What it does | Effect |
|---|---|---|
| Order sizing | "Scaled to budget": the ladder is sized to the whole budget. "Fixed USDT": you type the sizes | Scaled guarantees the ladder fits the budget |
| Max safety orders | How many extra buys | More orders last through deeper drops but each one is smaller |
| Deviation to first safety order | How far below the start the first extra buy sits | Small buys often, large starts deeper |
| Safety order step scale | Each gap as a multiple of the previous one | Above 1 the steps widen and the ladder reaches deeper |
| Safety order volume scale | Each order as a multiple of the previous one | Large pulls the average down fast but concentrates the budget deep down |
| Take profit | How far above the average to sell | Small is frequent small profit, large is rare large profit |
| Trailing take profit | At the take profit level it does not sell at once; it follows the peak | **The trailing deviation must be below the take profit.** In tests it mostly made results worse |
| Stop loss | Exits at market this far below the average. It must sit beyond the last safety order and before the liquidation price of every order; the form refuses a stop past liquidation. When you switch it on, the form proposes a value that fits | Caps the loss, but may sell at the bottom and miss the recovery |
| Max cycle duration | Sells the cycle at market when the time is reached; a running bot starts a new cycle after the cooldown | A short limit closes more cycles at a loss |

**A blank form** starts from the DCA Long Classic ladder, with the drawdown stop, the BTC break hold and the portfolio breaker on, so it is not that template. A note above the form names the templates that passed their checks; a blank grid form says 0 of 4 grid templates passed.

**Ladder coverage:** the Summary panel shows "Price coverage", how far below the start the last safety order sits. Coverage should be larger than the coin's past sharp drops.

## Grid settings one by one {#grid-settings}

| Setting | What it does | Suggestion |
|---|---|---|
| Price range | "Relative %": set around the start price every cycle. "Fixed prices": you type the lower and upper price | Relative for beginners |
| Lower / Upper | Width of the range | About the coin's swing over recent weeks |
| Grid intervals | How many steps | More steps mean frequent but smaller profit; per-step profit must stay above fees |
| Spacing | Geometric: equal percent. Arithmetic: equal price difference | Geometric |
| Grid mode | Neutral: no position at start. Long: buys at start. Short: sells at start | Neutral in a sideways market |
| Stop beyond range | Closes the grid once price trades this far beyond the range | Keep it on |
| Trailing up | In a long grid the range moves up with price | In a slowly rising market |
| Take profit on cycle profit | Closes once the cycle's profit reaches this share of the budget | Optional |

Check **"Profit per grid after fees"** in the Summary panel. Below 0.3% the steps are too tight and fees eat the profit.

## Start, restart and protection {#protection}

| Setting | What it does |
|---|---|
| Start condition | "Immediately" or "Price trigger": starts when price reaches your level |
| Cooldown between cycles | Minutes to wait after a cycle closes |
| Limit the number of cycles | The bot stops after this many cycles |
| Restart after a stop loss | When off, a stopped bot stays stopped |
| Price range guard | No new cycle while price is outside this band |
| End time | No new cycle after this date |
| Bot drawdown stop | Closes and stops once bot equity falls this share of the budget from its peak. If one candle touches both this stop and the take profit, the stop wins: the adverse move is assumed first |
| Hold new cycles on a BTC break | No new cycle while Sentinel reports a BTC break |
| Portfolio breaker | Closes every bot inside it once their combined loss reaches 15% |

**Protection cuts both ways in DCA.** DCA earns by holding through the drop and selling the bounce. Any protection that closes at the bottom can miss that bounce. In the `dca_long_classic` test the 2-year result fell from +41.5% to +29.8% with the portfolio breaker on. Turning protection on is your risk choice: less profit, a capped loss.

## Work out your risk {#risk}

Answer three questions before you start a bot:

1. **What do I lose in the worst case?** The Summary panel's "Worst case" line names the exit that comes first: at the stop loss, at the drawdown stop or at liquidation. It says "Open-ended: no stop" only when none of them exists. For a 1x long DCA with no stop the theoretical loss is the whole budget if the coin goes to zero.
2. **How long can the budget stay locked?** A DCA with no stop holds the budget until a recovery. Be ready to wait months.
3. **How far is liquidation?** For a leveraged DCA the Summary panel's "Last order to liquidation" line says how much further price can go after the last safety order.

**Budget rule:** depending on your risk level, all DCA and Grid budgets together cannot exceed a cap of 20% to 80% of your declared balance. The cap is on [Risk & safety](#/risk). A bot also has a **Minimum budget**, shown in the Summary: the smallest budget at which every order is at least 5 USDT.

## Backtest: try it on the past first {#backtest}

The [Backtest](#/backtest) page runs a setting on past prices with the same engine.

1. Choose the strategy type, coin and settings.
2. Choose the period (30 days to 2 years, or custom) and the candle interval. Shorter candles (15 min, 1 h) reflect moves inside a candle more accurately.
3. Press **Run backtest**.

### Reading the report

| Field | Meaning |
|---|---|
| Net | Total profit or loss at the end of the period, % of budget |
| Max drawdown | The deepest fall of equity from its peak, read at the closes of the chosen candle interval. Paper bots use 1-minute candles, so a longer interval shows less of the drawdown |
| Cycles | Closed cycles; "+1" means a cycle was still open at the end |
| Exit reason | Take profit, trailing take profit, stop loss, liquidation, drawdown stop |
| Coverage | How much of the requested candles the data had |

### Backtest traps

- **One period misleads.** In a rising month every long DCA looks good. Also test a period with a drop.
- **Tuning to the result.** Changing settings over and over on the same period to find the best result memorises the past, not the future. Try what you found on another period.
- **Trusting the win rate.** A bot that wins 24 cycles and is liquidated in one is at a loss overall.

## While a bot runs {#running}

| State | Meaning |
|---|---|
| Running | May open new cycles |
| Paused | Opens no new cycle; an open cycle runs on with its take profit and stop |
| Stopped | Opens no new cycle, no open cycle |
| Ended | Ended by liquidation or a spent budget; clone it to set it up again |

| Button | What it does |
|---|---|
| Start | Shown instead of Resume for a bot that never ran |
| Resume | Lets a paused or stopped bot open new cycles again |
| Pause | Stops new cycles, leaves the open one alone |
| Close cycle and stop | Closes the open cycle at market right away |
| Clone | Opens a new bot form with the same settings |
| Delete | Removes a stopped, flat bot from the list; its history is kept |

**Emergency:** **Close all** on [Positions](#/positions) closes every position and DCA/Grid cycle and stops the bots. For bots on real money, their Binance position is closed at market on the next pass (a few seconds). To close any position on your Binance account directly, use [Positions → Exchange](#/positions?tab=exchange).

**After the app restarts** paper bots that were running resume by themselves, oldest first, if they pass the Start checks again: leverage within the risk level's maximum, valid settings, the budget cap and the balance. A bot that fails a check stays Stopped with a note on its page ("Not resumed after the restart"). A bot whose open cycle cannot be read stays Stopped too, and a bot on real money comes back Stopped. Open cycles are still managed either way. A forced close that found no price is kept and tried again after the restart.

## Real money {#real-money}

Real money is optional and switched on per bot. Run the bot on paper first and watch how it behaves.

**What it needs**

1. The live build (the default build trades on paper only).
2. A verified **Binance trade key** in [Settings → Exchange keys](#/settings?tab=keys): Futures on, withdrawals off. Keys that can withdraw are refused. Real money is Binance only for now: Bybit and OKX have not yet passed an end-to-end run on their own test networks, so switching real money on for them is refused. Their keys still work for the account view and for closing positions.
3. The bot's market must be **Futures**.
4. A protective level: a stop loss for DCA, a stop-out for Grid, or a drawdown stop for either. The exchange stop rests at that level.
5. No open paper cycle, and no position on the symbol in your Binance account.

**How it works**

- The bot decides on 1-minute bars exactly as on paper. Every buy, sell and close goes to Binance as a **market order** (isolated margin, the bot's leverage). Taker fees apply, and waiting for the bar to close can add 1–2 minutes of delay.
- A **stop** always rests on Binance for the position. It protects the position even while the app is closed.
- The **Real money** panel on the bot page shows the real position, entry, exchange stop, realized result and last sync time. A **LIVE** badge shows in the top bar.

**Pilot: small first steps**

When you switch real money on, the bot starts with a pilot. For DCA and Grid, the first 3 real cycles send a reduced fraction of the simulated position to Binance (a position of about 100 USDT). Decisions and the stop level are the same; only the quantity is smaller. For a signal bot, the first 3 real entries are at most 50 USDT, raised to just above Binance's minimum for the symbol when that is higher. **End pilot** on the panel switches to full size; for DCA and Grid it applies from the next cycle.

**Real vs paper**

A DCA or Grid bot on real money gets a **Real vs paper** tab on its page. It compares every real fill with the simulation's fill at the same moment: the price difference (slippage, in basis points), its cost in USDT and the delay. Positive slippage is a cost. This difference is the main result of a test.

**Which signals:** the Futures bot takes only signals published for futures, the Spot bot only spot signals. A signal for the other market shows in the skipped trades with its reason.

**When the bot stops itself**

| Situation | What happens | What to do |
|---|---|---|
| The exchange stop filled | A normal stop loss; the bot stays armed | Nothing |
| The position closed on Binance another way (liquidation, manual close) | The cycle closes and the bot stops | Check why, then start the bot again |
| The Binance position differs from what the bot sent | The bot stops; the position stays under a stop | Check the position on Binance; close it from [Exchange](#/positions?tab=exchange) if needed |
| Binance could not be read for over 2 minutes while the bot traded | The bot does not catch up at today's price; it stops | Check the position and switch the bot back to paper |
| An order was refused 3 times | The bot stops | Check the balance and the minimum order size |
| Real money was switched on for an exchange that has not passed its test-network run | No new exposure; the real position stays under its stop and the bot cannot be started | Close the position on [Exchange](#/positions?tab=exchange) and switch the bot back to paper |

**While it runs, don't:** lock the vault, change keys or install an update. The app blocks these anyway. A bot on real money cannot be edited or deleted, and its exchange cannot change while it holds real positions: close its position and switch it back to paper first.

## Common mistakes {#mistakes}

1. **Raising leverage with the stop off.** One sharp drop takes the whole budget by liquidation.
2. **A trailing deviation close to or above the take profit.** A cycle that reached its take profit can close at a loss. The form now refuses equal or larger values.
3. **DCA on a small, newly listed coin.** These coins can head toward zero without recovering.
4. **A short ladder.** A 4-order ladder covering 15% runs out in an ordinary crypto drop.
5. **Grid in a trending market.** When the range breaks, the grid closes at a loss at the stop or holds inventory losing value.
6. **Backtesting one month and generalising.**

## Frequently asked questions {#faq}

**Can I lose real money?** On paper, no: no order reaches the exchange. If you switched a bot to [real money](#/guide?s=real-money), yes; every trade of that bot happens on your Binance account.

**Why did the stop loss not fire?** It may be off in the form. When off, the warning line says "No stop: loss is open-ended.", or "Only the bot drawdown stop limits the loss." when the drawdown stop is on.

**Why is the bot not opening a new cycle?** Check the notes on the bot page. Possible reasons: price range guard, end time, cycle limit, BTC break hold, portfolio breaker, budget cap, leverage above your risk level's maximum, the bot was not resumed after a restart, or a price feed older than 3 minutes.

**Why can't I delete or edit my bot?** Delete and settings are not in the bot list but on the bot's own page: click the bot's name; settings are on the **Settings** tab, **Delete** is among the buttons at the top. While a cycle is open, that is while the bot holds a position, delete is off and settings that would break the cycle, such as symbol and budget, are locked. Close the cycle first with **Close cycle**; then **Delete** is available. You can also create a new bot without deleting, if the budget cap has room and no other bot runs on the same symbol.

**What does "Paused: Price feed older than 3 minutes" mean?** The bot has had no new price for 3 minutes and waits rather than trade on an old one. It is usually a short outage of the exchange or the internet connection. The bot resumes by itself when prices return; pressing **Resume** does not fix it. If you press **Close cycle** meanwhile, the close also waits for a price, because the app never books a close at an old price. A new bot does not help during the wait: it uses the same price source.

**Why does profit look small?** DCA and Grid make small but frequent profits. Look at the monthly return, not one cycle.

**A template says "Did not pass". Can I use it?** Yes, but it missed at least one of the checks. The app asks before you use it. Try it in a backtest first.

**Which coin should I pick?** Start with the coins with the highest trading volume over the last 90 days. Most templates were tested on the top 5.

## Glossary {#glossary}

| Term | Meaning |
|---|---|
| Base order | The cycle's first buy (long) or sell (short) |
| Safety order | An extra buy or sell as price moves against you |
| Average entry | The weighted average price of all buys in the cycle |
| Cycle | One round from the base order to the close |
| Ladder | The prices and sizes of the base order and every safety order |
| Coverage | The largest price move the ladder can absorb |
| Step (grid) | The gap between two neighbouring grid prices |
| Inventory | The coin a grid holds |
| Unrealised P&L | The open position's profit or loss at the current price |
| Drawdown | Equity's fall from its peak to its lowest point |
| Liquidation price | The price at which a leveraged position is force-closed |
| Confidence interval | The range the true average most likely lies in |
