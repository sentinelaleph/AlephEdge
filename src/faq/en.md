# Frequently asked questions

This page walks through every sidebar menu in order: what it is for, what you see on screen and the questions people ask most. Every number in the screenshots is sample data; each picture carries a "Sample data" label. It is not your account or a trading record.

> For a full explanation of DCA and Grid, see the [Guide](#/guide). This page explains how the menus work.

## How the app is laid out {#layout}

![Dashboard: sidebar on the left, header on top, status bar at the bottom](shot:dashboard)

Every page uses the same layout:

- **Left sidebar:** the menus. Groups: Overview, Bots, Market, Research, Portfolio, Control. At the bottom: **Settings**, **Account**, the theme button and the collapse button.
- **Left panel:** the page's filters or settings.
- **Center:** the page's main work: tables, forms, charts.
- **Right panel:** summaries and status. In a narrow window the right panel hides; the **Summary panel** button in the header opens it.
- **Header (top):** page title and buttons, the mode label (**Paper** or **Paper + Live**), the **Testnet exchange** badge when the testnet is in use, the alerts bell, the Binance and Sentinel connection chips, and your account.
- **Status bar (bottom):** exchange connections and latency, the Sentinel signal stream and the age of the last signal, BTC price and regime, the vault (unlocked or locked), membership and the version number.

**Is the "Paper" label a button?** No, it only reports. It says **Paper** when every bot trades on paper and **Paper + Live** when at least one bot trades real money. Real money is switched on per bot.

**What do the dots on the Binance and Sentinel chips mean?** A green dot means the connection works. Click a chip to check it again right away. "no key" on the Binance chip means there is no Binance key in the vault; paper bots do not need one.

**What is the number on the alerts bell?** Unread alerts: budget or position limits full, stop loss hit, trades closed, DCA/Grid cycles closed today, DCA/Grid cycles closed by a stop, a DCA/Grid bot that ended, vault auto-lock, stream down, BTC regime changes. Choose which alerts you get under **Settings → Notifications**.

**What are the numbers and dots in the sidebar?** The number next to a bot menu is the count of running bots. The number next to **Positions & orders** counts open positions and open cycles. The green dot next to **Signals** means the signal stream is connected. A dot next to **Risk & safety** means the daily stop tripped or the BTC regime is not normal; a **LIVE** chip there means at least one bot trades real money.

**How do I change the theme or collapse the sidebar?** The theme button at the bottom of the sidebar cycles Light → Dark → High contrast → System. The arrow next to it collapses the sidebar; Ctrl+B does the same.

**Why does the lock screen appear?** The vault (the encrypted store of your exchange keys) locks itself after a set time without activity: no mouse, keyboard or scroll input in the window. A vault that holds no key stays unlocked. Bots keep running while it is locked; the lock protects only the interface and the keys. Choose the duration under **Settings → Exchange keys → Auto-lock**: from 5 min to 1 month. While real money holds an open position the vault does not lock itself.

**What happens if I open the app a second time?** Only one copy runs. Opening it again brings the open window to the front. The TESTNET build and the release build are separate apps and can run side by side.

## Dashboard {#dashboard}

The Dashboard shows on one screen what is running, what it made or lost and what needs attention. Nothing the app does not have is estimated; without data a field stays empty.

- **Left panel:** **Scope** (paper and live bots running), **Bots** (how many of each type run), the **BTC regime** and **Signal stream** cards.
- **Center:**
  - **Getting started** checklist: signed in to Sentinel, a bot configured, a bot running. Any bot type counts. With no bot yet, it points to DCA Long Classic, a template that passed its test. The signal stream step appears once a signal bot is configured. Once all are done, **Hide checklist** removes it.
  - Summary tiles: **Realized today**, **Net P&L, all time**, **Daily loss used**, open positions, **Capital in use**, **Signal win rate**.
  - **Configured bots** table: start and stop any bot from here.
  - **Recent trades**: the last 10 closed trades.
- **Right panel:** **Alerts**, the **Daily stop** card and, when an exchange account is connected, the **Exchange account (real)** panel.

**Do the P&L tiles include DCA and Grid bots?** Yes. **Realized today** and **Net P&L, all time** add the closed DCA/Grid cycles, after fees and funding, deleted bots included. The note under each tile splits it: "Signal x · DCA/Grid y". **Signal win rate** and **Daily loss used** count signal bots only.

**Why does the signal win rate say "too few to judge"?** With fewer than 30 closed signal trades a win rate is not reliable; the app says so.

**What does "Daily loss used" measure?** Today's realized loss of the signal bots as a share of the daily stop limit. The limit comes from the simulated balance and the daily stop % on the Risk & safety page. Unrealized losses of open positions do not count. DCA/Grid cycles do not count either: those bots are outside the daily stop.

## All bots {#all-bots}

![All bots: filters on the left, signal bots and DCA and Grid bots in the center](shot:bots)

Every bot of every type (signal, DCA, Grid) in one table.

- **Left panel (filters):** search (bot name or symbol), type, state, market (Spot/Futures), mode (Paper/LIVE), exchange and **Clear filters**. Filters are kept in the address.
- **Center:** signal bots and **DCA and Grid bots** in separate sections. You can select several signal bots and stop them together.
- **Right panel:** a summary per bot type and **Needs attention**: signal bots stopped by the daily stop, paused or ended DCA/Grid bots.

**Can I create a new signal bot?** No. There are three fixed signal bots: **Futures**, **Spot** and **Pump**. You change their settings, start or stop them. You can create up to 10 DCA and Grid bots.

**Why can't I delete or edit a bot?** Each paper bot's row has a trash icon that deletes it. Settings are on the bot's own page: click the bot's name. Details in the [DCA bots](#/faq?s=dca-bots) section.

## Market trend {#market-trend}

The **BTC trend** chip in the top bar and the **Market trend** panel on the bot pages show BTC's slow trend. They are information only: no bot reads them and nothing changes by itself.

- **How it is read:** BTC's daily close against its 50-day average. Above is **Up**, below is **Down**. The direction changes only after 2 daily closes in a row on the other side. Source: Binance daily BTC/USDT closes, read every 15 minutes.
- **Since / days:** when the current direction was confirmed.
- **Distance to the average:** how far the last close is above or below the 50-day average.
- **Direction changes, last 12 months:** how often the trend flipped. A high number means the trend often turns before a move pays.
- **On the create forms:** a note shows the trend now, and warns when the bot's side runs against it (a long bot in a downtrend, a short bot in an uptrend).

**Why don't the bots follow the trend by themselves?** We tested exactly that on two years of data before building anything (8 Oct 2026, rules written before the run). For DCA, not starting new deals in a downtrend cut the worst bot's drawdown by 5 to 7 points, and cut the return by about a third in the test period. For signal bots, trades with the trend lost less than trades against it, but they still lost money in 2 of 3 periods. Neither result was strong enough to let the app trade on it, so the app shows the trend and you decide.

## Signal bots {#signal-bots}

![Signal bots: risk level, BTC regime and signal stream on the left; bot table, open positions and skipped trades in the center](shot:signal-bots)

The three bots that trade Sentinel's published signals.

- **Futures:** takes futures signals, long and short.
- **Spot:** takes spot signals, long only.
- **Pump:** takes futures signals from Sentinel's pump engine that also show a structure break (MSB or CHoCH) and an order block or FVG. It runs on paper only, at any risk level, within that level's limits. It is marked **Untested**: no replay or forward test of this rule exists.

**Short signals:** Sentinel stopped publishing short signals on 8 Oct 2026, when its own pre-registered kill rule tripped. Until shorts return under a pre-registered rule, a bot whose **Direction** is **Short** receives no Sentinel signals, and the Futures and Pump bots receive longs only.

On screen:

- **Left panel:** risk level summary, **BTC regime**, **Signal stream** and feed filters (**Feed: bot**, **Feed: skip reason**).
- **Center:** the bot table, **Open positions**, **Skipped trades** and the selected bot's settings form.
- **Right panel:** trades opened and closed today, **Skip reasons** counts and **How this bot works**.

![A signal bot's settings form](shot:settings)

Sections of the settings form:

- **Bot:** exchange, **Capital per position (USDT)** with your risk level's cap shown under it, **Max concurrent positions** and leverage (Futures and Pump).
- **Position sizing:** **Risk per trade** or **Fixed size**.
- **Take profit:** TP1, TP2, TP3 or a custom target.
- **Signal filter:** **Max signal age (hours)**, **Min Sentinel score (%)**, **Direction**, **Max loss per position (%)**, **Symbol whitelist**, engine whitelist and the setups the bot may trade. Saved filter changes also apply to pending signals the old settings skipped.

**The bot runs but opens no trades. Why?** The bot never skips a signal silently: every skipped signal is written to **Skipped trades** with its reason. The **Skip reasons** panel on the right shows how often each reason occurs. The most common ones:

| Skip reason | Meaning | What to do |
|---|---|---|
| Above per-position cap | The bot's capital per position is above the share of the simulated balance your risk level allows (2% at Cautious). Such a value is refused at save and at start; this skip appears when the balance or the level was lowered while the bot ran | Lower **Capital per position**, or raise the balance or the level on Risk & safety |
| Below the exchange minimum order | The position would be under about 5 USDT, the smallest order Binance accepts on most pairs, so the trade could not exist on the exchange and the paper book refuses it too. Common on small balances: 100 USDT at Cautious allows 2 USDT per position | Raise the balance or the level on Risk & safety, or set **Capital per position** to at least 5 USDT |
| Position limit reached | This bot's open positions reached the level's maximum (the level limit applies to each signal bot separately: Spot, Futures and Pump) | Wait for positions to close, or raise the level |
| Bot position limit reached | This bot's **Max concurrent positions** is full | Raise the limit |
| Published for another market | A spot signal reached the Futures bot (or the other way round) | Nothing; each bot takes its own market's signals |
| Signal older than this bot's age limit | The signal is older than **Max signal age** | The default is 4 hours; 0 removes the limit |
| BTC breakdown, new longs paused | The BTC regime is "Break": no new longs, shorts continue | Wait for the regime to return to normal |
| Combo not in whitelist | The signal's setup is not on the bot's allowed list | Widen the setup list in the settings form |
| Signal already resolved | The signal already reached its target or stop; too late to enter | Nothing |
| Below the Sentinel score filter | The signal's score is below **Min Sentinel score** | Lower the filter if you want these signals |

**Skipped trades** and the **Export skipped signals** file on the History page keep one row per bot, signal and reason, timed at the first sighting.

**What is the difference between capital per position and the simulated balance?** **Capital per position** is what this bot puts into one position. The **simulated balance** is the total balance on the Risk & safety page, and every limit derives from it. Example: at Cautious with a 5000 USDT balance, one position may use at most 2% of it, 100 USDT. If you set the bot's capital to 5000, the app refuses to save it and to start the bot. A new bot's capital is 100 USDT, lowered to the cap when the cap is smaller: 20 USDT on a fresh install (Cautious, 1000 USDT balance).

**How is risk per trade sized?** The position is sized so that hitting the stop loses the chosen % of capital. Example: capital 100 USDT, risk 1%, stop 3% away gives a position of about 33 USDT. A position is never larger than capital × leverage; when that cap binds, a **Capped** chip appears and the real risk is below what you chose.

**What does Max loss per position do?** It closes a position once its loss reaches this % of the bot's capital, after leverage and fees. Empty means off. The line under the field says what your value does: with **Fixed size**, the price move against the position that closes it; with **Risk per trade**, no effect when the value is at or above the risk %, because the stop closes first.

**What does the Sentinel score measure?** It is Sentinel's 0–100 signal score: setup, indicators, confluence and BTC regime, weighted. It is not a probability. Tested on past signals, it did not separate winners from losers (AUC 0.496). The **Min Sentinel score** filter still works as set.

**Why does the Pump bot say "Untested"?** No replay or forward test of its rule exists. It runs on paper only, at any risk level, within that level's limits. Its page shows the full rule. It cannot switch to real money.

**At what price does a paper trade close?** Stops, targets and partial exits book at their own level. If the app was not watching when a stop was crossed (a restart, the computer asleep), the stop books at the first price seen after the gap.

**What is the daily stop?** When today's realized loss reaches the level's share of the simulated balance (2% at Cautious), every signal bot stops. If **Close positions on daily stop** is on, open positions close too. Bots can be restarted after 00:00 UTC. DCA and Grid bots are outside the daily stop; they have their own protection.

**How do I switch a signal bot to real money?** Only the **Futures** bot, only on Binance, and only in a build with real money. In the **Real money** panel under the settings form, press **Switch to real money**, type LIVE and confirm. You need a verified trade-only Binance key in the vault. The first 3 real entries run as a pilot of at most 50 USDT each. While the bot is on real money or holds real positions, its exchange cannot change. When the app restarts, the signal bot switches back to simulated on its own.

**What happens to signal bots when the app restarts?** Paper bots that were running start again by themselves. A bot stays stopped when its capital is above the per-position cap, its saved settings fail a check, its saved data cannot be read or the daily stop has tripped; **Skipped trades** names the reason ("Not resumed after restart"). A bot on real money comes back stopped and on paper.

## A signal bot's page {#signal-bot-detail}

![A signal bot's page: overview and open positions](shot:bot-detail)

Opens when you click a bot's name. Tabs: **Overview** (open positions), **Trades** (closed trades and CSV), **Orders** (each open position's take profit and stop loss), **Settings** and **Log** (skip notes).

**Can I close an open position by hand?** Yes, with **Close** on the position's row. A paper position is booked at the live price. On a real-money position a real market order goes to the exchange, so you are asked to type CLOSE.

## DCA bots {#dca-bots}

![DCA bots list](shot:dca)

A DCA bot buys in steps as the price falls during a cycle (safety orders), lowering its average cost, and sells the whole position at the take profit. The full explanation is in the [Guide](#/guide?s=dca).

- **List:** name, symbol, market, state, budget, total P&L, realized, drawdown, cycles. The row button is **Pause** while the bot runs, **Resume** for a paused or stopped bot and **Start** for a bot that never ran.
- **Header buttons:** **DCA presets**, **Export CSV** and **New DCA bot**.
- **Right panel:** the bots' summary and **Strategy budget and breaker**: declared balance, strategy budget cap, **Reserved by bots**, **Free under the cap**, max leverage and the portfolio breaker.

![A DCA bot's page: overview, open cycle and budget panel](shot:dca-detail)

Click a bot's name to open its page:

- **Tabs:** **Overview**, **Cycles**, **Orders**, **Fills**, **Settings**, **Log**. A bot on real money also gets **Real vs paper**.
- **Buttons:** **Start** (a bot that never ran) or **Resume**, **Pause**, **Close cycle and stop**, **Clone**, **Delete**.
- **Right panel:** the bot's state, the **Real money** panel and the strategy budget.

**What do the bot states mean?**

| State | Meaning |
|---|---|
| Waiting for entry | Running, no open cycle; waiting for its start condition |
| In cycle | Running with an open cycle |
| Paused: managing open cycle | No new cycles; the open cycle is managed until its take profit or stop |
| Paused: Price feed older than 3 minutes | No new price for 3 minutes; resumes by itself when prices return |
| Stopped | Opens no new cycles and holds no cycle |
| Ended | Finished for good after a liquidation or losses; **Clone** sets it up again |

**What is the difference between Pause, Close cycle and stop, and Delete?**

- **Pause:** no new cycles. An open cycle keeps being managed until its take profit or stop.
- **Close cycle and stop:** the open cycle closes at market right away (fees and slippage included) and the bot stops. You type CLOSE to confirm.
- **Delete:** removes the bot from the desk. Its cycles and fills stay in the CSV export. You type DELETE to confirm.

**Why can't I delete or edit my bot?** Delete is the trash icon on the bot's row in the list, and **Delete** on the bot's page. If a cycle is open, the dialog shows its size and open P&L; typing DELETE closes the cycle at market, stops the bot and removes it. While a cycle is open, only name, start, restart and protection settings can change; symbol, budget, leverage and strategy settings are locked. To change them now, use **Close cycle and stop**, edit, then **Start**. A bot on real money cannot be deleted until it is back on paper.

**What does "Paused: Price feed older than 3 minutes" mean?** The bot has had no new price for 3 minutes and waits rather than trade on an old one. It is usually a short outage of the exchange or the internet connection. When prices return the bot resumes by itself and processes the missed minutes in order; pressing **Resume** does not fix it. Closing the cycle is not possible either until a price arrives, because the app never books a close at an old price. A new bot does not help: it uses the same price source.

**What happens to my bots when I restart the app?** Paper bots that were running resume by themselves, oldest first, but only if they pass the Start checks again: leverage within the risk level's maximum, valid settings, the budget cap and the balance. A bot that fails a check stays stopped, and its page says why ("Not resumed after the restart"). A bot whose open cycle cannot be read stays stopped too. A bot on real money comes back stopped and waits for you. Open cycles are not lost: the closed time is processed and the cycle keeps being managed. A forced close that found no price before the restart is kept and tried again.

**What is the budget cap? I get "Over the DCA/Grid budget cap".** The budgets of all DCA and Grid bots together cannot exceed a share of the simulated balance set by the risk level:

| Risk level | Cautious | Calm | Balanced | Ambitious | Greedy |
|---|---|---|---|---|---|
| Total DCA/Grid budget | 20% | 30% | 40% | 60% | 80% |

Example: on a fresh install (1000 USDT simulated balance, Cautious), all DCA/Grid bots together may reserve at most 200 USDT. The error shows what is free and the cap. To raise it, increase **Simulated balance (USDT)** or the risk level on Risk & safety. Stopped bots with no cycle reserve nothing.

**What is the minimum budget?** The smallest budget at which every order of the bot is at least 5 USDT. The **Summary** panel shows it as **Minimum budget**. DCA Long Classic needs 176.98 USDT and DCA Long Safe 218.10 USDT, so on a fresh install (200 USDT cap) Safe does not fit. The create page lowers the opening budget to what is free under the cap when that still clears the minimum; otherwise a banner names the minimum and links to Risk & safety.

**"Another bot already runs this pair, market and side"?** Only one bot may run on the same pair, market and side at a time; otherwise their orders would mix. Pause the other bot or choose another pair.

**Why can't I raise the leverage?** Leverage is capped by the risk level: Cautious 2x, Calm 3x, Balanced 5x, Ambitious 10x, Greedy 20x. Spot is always 1x. If you lower the level below a bot's leverage, that bot holds new cycles ("Leverage above your risk level's maximum").

**How do I switch a DCA bot to real money?** Only in a build with real money, and only for Binance Futures bots. In the bot's **Real money** panel, the three items under **Before real money** must pass: a verified Binance trade key, a stop loss (or drawdown stop) and no open paper cycle. Then press **Switch to real money** and type LIVE. The first 3 cycles run as a pilot at a small size (a position of about 100 USDT); **End pilot** switches to full size. Real money stays on after the app restarts. While the bot is on real money or holds real positions, its exchange cannot change.

**What does the "Real vs paper" tab show?** Every real fill next to the simulation's fill at the same moment: the price difference (slippage), its USDT cost and the delay. Positive slippage is a cost.

## Grid bots {#grid-bots}

![Grid bots list](shot:grid)

A grid bot places evenly spaced buy and sell orders across a price range and collects small profits as the price moves back and forth inside it. The full explanation is in the [Guide](#/guide?s=grid).

![A grid bot's page](shot:grid-detail)

The list, the bot page, the buttons, the states, the budget cap and the delete rules are the same as for DCA bots; the [DCA bots](#/faq?s=dca-bots) section applies to Grid too. Grid-specific:

**What does "Paused: Price outside the grid range" mean?** The price left the bot's trading range. Grid orders work inside the range, so the bot waits and resumes when the price comes back.

**Why are there no fills at all?** A fill needs the price to move by the distance between grid levels. In a quiet market hours without a fill are normal. The **Orders** tab on the bot's page shows the waiting buy and sell levels.

**Which grid templates passed the tests?** None. Only two DCA templates passed: DCA Long Classic and DCA Long Safe. Grid templates are there to learn and try; try them in a [Backtest](#/faq?s=backtest) first.

## New bot {#new-bot}

![New bot: the three bot types](shot:bot-new)

The page where you choose a bot type. Three cards: **Signal bot (Sentinel)**, **DCA bot** and **Grid bot**. Each card has a **Create** button; the DCA and Grid cards also have **Presets for this type**. The N key opens this page too.

![The DCA bot form](shot:dca-new)

The DCA or Grid form:

- **Bot:** name, market, direction (DCA) or grid mode, pair, **Budget**, **Leverage** and **Margin mode** (always isolated).
- **Orders and exits** (DCA) or **Grid** settings.
- Start and restart settings.
- **Protection:** **Bot drawdown stop**, **Hold new cycles on a BTC break**, the portfolio breaker.

The **Summary** panel on the right shows the minimum budget, whether the budget fits under the cap, the order ladder, the estimated liquidation and the worst case. For the meaning of each setting, use the "What does each setting do?" link above the form.

A blank **New DCA bot** form starts from the DCA Long Classic ladder, with the drawdown stop, the BTC break hold and the portfolio breaker on. A note above it names the templates that passed their checks; a blank grid form says 0 of 4 grid templates passed.

**What is the difference between Create and Create and start?** **Create** saves the bot and leaves it stopped; its budget must fit under the cap. **Create and start** saves and starts it; its budget must fit in what is free under the cap now. If the start is refused, the bot is still created and its page shows the reason.

**Why does the app ask me to confirm "Liquidation possible, no stop"?** The summary shows a liquidation price and nothing closes the position before it: the stop loss (DCA) or **Stop beyond range** (grid) is off. That includes a short DCA at 1x: a 1x short is liquidated when price roughly doubles.

**Why is my stop loss refused?** A DCA stop loss must sit beyond the last safety order and before the liquidation price of every order in the ladder; a stop past liquidation would never fire. When you switch the stop loss on, the form proposes a value that fits.

**Why does Create on the signal bot card not create a bot?** The signal bots are the three fixed bots; the card takes you to **Signal bots**.

## Signals {#signals}

![Signals: filters on the left, the signal table in the center, the selected signal on the right](shot:signals)

Sentinel's published signals as a table. Each row and the selected signal's panel have an **Execute** button.

- **Left panel (filters):** symbol, direction, timeframe, state (active or expired), **Min. score** and age.
- **Center:** time, symbol, timeframe, side, entry, TP1–TP3, stop loss, **Score**, setup, state and age.
- **Right panel:** the selected signal's detail (entry, targets, risk/reward, confluence, **Management plan**) and **Bot decisions**: what your bots did with it.

**What does Execute do?** It opens a window for the signal with these sections, in order. **Take this signal now · paper** opens the signal as a paper position on a fitting signal bot: Futures for a futures signal, Spot for a spot long, Pump when it is configured and the signal is a futures one. It enters at the current price, also when the signal's entry has passed, as long as the price is between the stop and the bot's target; past the target or the stop it is refused. Before the click each bot shows the fill at the current price, the signal's entry, the reward:risk at the fill next to the published one (a warning below 0.5: the trade has moved), the position size, and the loss at the stop and the gain at the target, both with fees. **Open position** opens it; the bot does not have to be running, it needs saved settings. The bot's own filters are skipped (age limit, Min. score, direction, symbols, setups, engines, the Pump rule): you decided. Its risk limits stay: position limits, per-position cap, the exchange minimum, the liquidation check, the daily stop, funding and depth, the BTC guard. A refusal shows its reason. The position carries a **Manual** label on Positions & orders, the bot page and in History. Execute never opens real money: a bot switched to LIVE refuses. Below that: a link to the signal bots, which also take Sentinel signals by themselves within their filters; the DCA and Grid templates that fit the signal's side (a long signal shows long DCA and long or neutral Grid templates), each with its verdict and test mean per bot-month; and a blank DCA or Grid form. **Open with** fills the create form with the template and the signal's pair. A template that did not pass asks first. A BTC-only template appears only for BTCUSDT. The templates were tested on their own pairs, not on this one, and the bot runs on paper unless the build is live and LIVE is typed.

**Is the score a probability?** No. It is Sentinel's 0–100 signal score: setup, indicators, confluence and BTC regime, weighted. It is not the chance of reaching the target. Tested on past signals, it did not separate winners from losers (AUC 0.496).

**Why are there no short signals?** Sentinel stopped publishing short signals on 8 Oct 2026, when its own pre-registered kill rule tripped. Shorts return only under a pre-registered rule.

**Why are there no signals on USDC or PAXG?** Sentinel no longer scans stablecoins, fiat, gold and wrapped tokens (USDC, FDUSD, EUR, PAXG, XAUT, WBTC and similar).

**Why didn't a signal enter my bot?** **Bot decisions** only shows positions opened and closed. Skip reasons are in **Skipped trades** on the Signal bots page and in the bot's **Log** tab.

**What happens when the signal stream drops?** Signal bots receive no new signals; the Dashboard and the Signal bots page show a warning. **Reconnect** retries right away. DCA and Grid bots do not depend on the signal stream.

## Presets {#presets}

![Presets list](shot:presets)

DCA and Grid templates tested on past prices. Each has its test results and a verdict. Every figure is per bot-month: a new 1000 USDT bot per pair each month, with a cycle still open at month end left to run on until it closes. It is not one bot kept running for months.

- **Passed checks:** all four checks passed: profit in every data period, the test interval above zero, every test month positive, no liquidation.
- **Under review:** passed all four checks on the original test window, but a later re-run with a partial month did not confirm every check. The template stays, with that month shown, until the dated re-read.
- **Did not pass:** at least one check failed; the failed one is named.

![A preset's results page](shot:preset)

Click a template to see its settings, results per data window and the test method. **Use preset** fills the form with it; **Backtest** opens it on the Backtest page.

**Can I use a template that says "Did not pass"?** Yes. The app first shows which checks failed and asks you to confirm with **Use anyway**. Try it in a backtest first.

**What if I change a template's settings?** Name and budget are free. If you change any other setting, the template's results no longer describe your bot; the form says so and the bot is saved without the preset link. The pair matters too. The banner names the pairs the template was tested on. BTC-only templates need BTCUSDT; on any other pair the link is dropped. For templates tested on a ranked list (for example the top 5 by volume, or the altcoin template's pairs ranked 6 to 15), the banner says your pair is not checked against that list.

**What is the "Ghost since 8 Oct" column?** Since 8 Oct 2026 the server runs all 12 templates forward, every 6 hours, with the same simulator and settings as their test: a new 1000 USDT bot per pair each month, 1x, the month's coin list, fees, slippage and funding included, open cycles valued at the last close. Nobody's money is in them; they show how each template does after its test ended. The column is the total return since the start, not per month; the test column is per bot-month. For the first 30 days the column says **Collecting** and nothing is ranked: a few weeks mostly show what the market did. The template's own page shows the numbers from day one, with that note.

**Do these results show future profit?** No. They are a simulation on past prices with the costs shown; not a trading record and not a forecast.

## Backtest {#backtest}

![Backtest: run settings on the left, past runs in the center](shot:backtest)

Try a DCA or Grid setup on past prices. It is the safest way to test before real money.

- **Left panel (Run settings):** bot type, **Interval** (15m, 1h, 4h, 1d), **Window** (30 days to 2 years, or custom dates; at most 3 years) and all the bot's settings.
- **Run backtest** starts it; candle download progress is shown meanwhile. One run at a time.
- **Runs:** the last 50 runs, kept on this computer. Each row shows the net result, max drawdown, cycles and data coverage.

![A backtest report](shot:backtest-report)

The report shows net P&L, closed cycles, max drawdown, fees, the deepest safety order, liquidations, the equity chart and the cycles table. **Edit and rerun** opens the form with the same settings.

**Why do I need to sign in to backtest?** Historical candles come from the Sentinel server with your session.

**The report's "coverage" is low. Is that a problem?** Coverage is the share of requested candles received. Below 95% a warning appears; a result on missing data is less reliable.

**Does a backtest show exactly what the bot would do?** No. Fees are included, but futures funding, the BTC break rule and risk gates are not simulated. The report labels this. The drawdown is read at the closes of the chosen candle interval, while paper bots use 1-minute candles; a longer interval shows less of the drawdown.

## Guide {#guide}

![The Guide page](shot:guide)

The in-app handbook for DCA and Grid bots: how they work, worked examples, every setting, risk math, backtesting, real money and a glossary. The **Contents** list on the left moves between sections; the "What does each setting do?" links in forms take you to the matching section. The guide is in English and Turkish; other languages open the English text.

## Positions & orders {#positions}

![Positions & orders: Paper tab](shot:positions)

Everything that is open. Paper and real money are on separate tabs and never mix.

- **Paper tab:** signal bots' open positions and DCA/Grid bots' open cycles. Filters on the left (symbol, bot, direction); totals and exposure of the declared balance on the right.
- **Exchange tab:** the real positions on each exchange whose key is in the vault.

![Exchange tab: real positions on the Binance account](shot:positions-exchange)

**Does the Close button send a real order?**

- **On the Paper tab:** no. The position is booked at the live price. **Close all** closes every paper position and DCA/Grid cycle and asks you to type CLOSE. If a DCA/Grid bot is on real money, its Binance position is closed too; the dialog says so.
- **On the Exchange tab:** yes. Every **Close** and **Close all** sends a real reduce-only market order to the exchange that holds the position and asks you to type CLOSE. It works on Binance, Bybit and OKX. It is the one manual write the app allows in every build.

**When does a close count as done?** Only when the account reads flat on that symbol. If a remainder is left, the app sends one more reduce order; the stop is not cancelled and nothing is booked until the account is flat. **Close all** tries every position, even after one fails, and then lists the positions still open.

**Why are the buttons on the Exchange tab grey?** When the account snapshot is older than 60 seconds, the close buttons wait for a fresh one; no real order goes out on stale data.

**What does a "No exchange stop" chip mean?** The exchange did not confirm a protective stop for a real position. The app works to protect it; when you see this chip, check the position on the exchange as well.

## History & stats {#history}

![History & stats](shot:history)

Closed trades and performance stats.

- **Left panel (filters):** symbol, range (today, 7 days, 30 days), bot, direction and exit reason.
- **Center:** **Net PnL**, win rate, **Profit factor**, **Max drawdown**, trades and today; the trade table below; then the **DCA/Grid cycles** section with the closed cycles, after fees and funding, deleted bots included.
- **Right panel:** counts by bot and by exit reason.
- **Export CSV** in the header writes the trades to a file.

**Why don't the stats change when I filter?** Filters narrow only the trade list; the stats cover the whole scope. The page says so.

**Are real and simulated trades counted together?** No. In a build with real money, **Statistics scope** switches between **Simulated** and **Real money**; the two are never added together.

## Risk & safety {#risk}

![Risk & safety: live readiness, risk level and level limits](shot:risk)

Every limit and safety control in one place. Changes apply at once. The **Sections** list on the left jumps to each part.

**What do the risk levels change?**

| Level | Max leverage | Max positions (per signal bot) | Capital per position | Daily stop | DCA/Grid budget |
|---|---|---|---|---|---|
| Cautious | 2x | 3 | 2% | 2% | 20% |
| Calm | 3x | 5 | 4% | 4% | 30% |
| Balanced | 5x | 8 | 6% | 6% | 40% |
| Ambitious | 10x | 12 | 10% | 10% | 60% |
| Greedy | 20x | 20 | 15% | 15% | 80% |

Every percentage is of the **Simulated balance (USDT)**. Picking Greedy shows a warning and asks for **I understand the risk**. The level does not lock the Pump bot: Pump runs on paper at every level, within that level's limits.

**Can I set my own daily loss limit?** Yes, but only a stricter one. If you type a looser value than the level's cap, the level's cap stays in force.

**What does the live readiness check do?** It appears only in a build with real money. It reads everything real money needs in one pass: live build, Binance endpoint, trade-only key, your clock against Binance, futures balance, one-way position mode, positions no bot holds, daily stop, BTC regime, membership, signal stream and halted live bots. Each row is marked **OK**, **Check** or **Blocks**; a row with a problem has a **Fix** link to where it is fixed. It only reads; no order is sent. **Check now** runs it again.

**What is the BTC regime gate?** Sentinel's BTC reading. **Normal**: no block. **Break**: new long entries are paused, shorts continue. **Unknown**: entries that need a regime reading are refused.

**What do I do in an emergency?** In the **Emergency** section:

- **Stop all bots:** stops every running signal bot. No new entries; open positions stay managed.
- **Pause all DCA and Grid bots**.
- **Close all DCA and Grid cycles** (type CLOSE).

To close real positions on an exchange use **Positions & orders → Exchange**.

## Settings {#settings}

![Settings: General tab](shot:settings-page)

Tabs: **General**, **Notifications**, **Devices**, **Exchange keys**, **About**.

- **General:** **Language** (with number and date previews), **Theme** (Light, Dark, High contrast, System) and **Keyboard shortcuts**. For example: Ctrl+1…9 opens the menus, N opens a new bot, Ctrl+B collapses the sidebar.
- **Notifications:** which **In-app alerts** you see.
- **Devices:** pairing for phone remote control.
- **Exchange keys:** your API keys and the vault.
- **About:** version and updates.

![Exchange keys tab: stored keys and the vault](shot:accounts)

**Which Binance key should I add?** A key with trade permission only, **withdrawals off** and futures permission on. The app verifies the key's permissions with Binance when you add it and refuses keys that can withdraw. The key and secret are kept only on this computer, in the encrypted vault; they are never sent to a server.

**Which exchanges can place real orders?** Binance futures only, for now. Binance orders were run end to end on the Binance test network. Bybit and OKX have not yet passed that run on their own test networks: switching real money on for them is refused. Their keys still work for the account view and for closing positions on the **Exchange** tab. On Bybit and OKX the breakeven move changes the stop on the exchange in place; like every other order call it runs in the test-network dry run before either exchange is opened for real money. Bitget is not an order exchange: its keys are not accepted, because the app cannot check their permissions. Bot decisions use Binance prices on every exchange. The "Exchanges" count in the status bar is the exchanges whose prices are read.

**What is the vault, and what if I forget its password?** The vault is the encrypted store of your keys on this computer. There is no password recovery: if you forget it, **Reset vault** deletes the vault and every key in it, and you add your keys again. While real money holds an open position the vault cannot be locked or reset and keys cannot be changed.

**How do I change the auto-lock time?** In **Auto-lock** on the **Exchange keys** tab: 5 min, 15 min, 30 min, 1 h, 4 h, 8 h, 12 h, 24 h, 1 week or 1 month. The time counts from your last activity in the window. A vault that holds no key stays unlocked. Long durations mean the keys stay open as long as the computer stays on.

![Devices tab: phone pairing](shot:settings-devices)

**What can I do from my phone?** A paired phone can see the bots' status, stop the bots and close everything; the work stays on this computer. **Create QR** makes a single-use code valid for 2 minutes. Do not share the code or take a screenshot of it.

![About tab: version and updates](shot:settings-about)

**How do updates arrive?** The app checks shortly after launch and every 6 hours. When a new version exists, **Install and restart** installs it. After any restart, positions and cycles are restored, and paper bots that were running carry on by themselves if they pass their start checks again; the [DCA bots](#/faq?s=dca-bots) and [Signal bots](#/faq?s=signal-bots) sections list them. A bot on real money comes back stopped and waits for you. Every update is verified against the signing key built into the app before it installs. Updates do not install while real money is open.

## Account {#account}

![Account page: Sentinel membership](shot:account)

Your Sentinel (ribqa.com) membership: name or email, membership state, plan, valid until and last refresh. **Refresh** reloads the membership; **Sign out** ends the session.

**What does the app need to work?** Three things: signed in to Sentinel, an active membership and an unlocked vault. If one is missing the app shows the matching screen: sign-in, membership or vault unlock.

**What happens to my bots if I sign out?** The Sentinel signal stream disconnects, so signal bots receive no new signals. The bots keep their state.

**I also use the TESTNET build. Why does it ask me to sign in?** Each build keeps its own sign-in and its own vault entry in Windows Credential Manager, so signing in, signing out or resetting the vault in one build does not touch the other. Sign in once in the TESTNET build. Its vault moves to the build's own entry the first time you unlock it there.
