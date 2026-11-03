/**
 * Golden scripts: written the way people paste them from TradingView.
 * Expected outputs are asserted in golden.test.ts against the independent
 * reference fixture or against independent computations in the test.
 * These are also the candidates for the owner's manual side-by-side check
 * against TradingView before launch (spec section 6).
 */

export const RSI_BANDS = `//@version=6
indicator("RSI with bands", shorttitle = "RSI", format = format.price, precision = 2)
length = input.int(14, "RSI Length", minval = 1)
src = input.source(close, "Source")
upper = input.int(70, "Overbought", maxval = 100)
lower = input.int(30, "Oversold", minval = 0)
r = ta.rsi(src, length)
plot(r, "RSI", color = #7E57C2)
h1 = hline(upper, "Upper Band", color = #787B86)
h2 = hline(lower, "Lower Band", color = #787B86)
fill(h1, h2, color = color.rgb(126, 87, 194, 90), title = "Background")
bgcolor(r > upper ? color.new(color.red, 85) : na)
`;

export const MACD_COLORS = `//@version=5
indicator(title="MACD", shorttitle="MACD", timeframe="")
fast_length = input(title="Fast Length", defval=12)
slow_length = input(title="Slow Length", defval=26)
src = input(title="Source", defval=close)
signal_length = input.int(title="Signal Smoothing",  minval = 1, maxval = 50, defval = 9)
col_grow_above = input(#26A69A, "Above   Grow", group="Histogram", inline="Above")
col_fall_above = input(#B2DFDB, "Fall", group="Histogram", inline="Above")
col_grow_below = input(#FFCDD2, "Below Grow", group="Histogram", inline="Below")
col_fall_below = input(#FF5252, "Fall", group="Histogram", inline="Below")
fast_ma = ta.ema(src, fast_length)
slow_ma = ta.ema(src, slow_length)
macd = fast_ma - slow_ma
signal = ta.ema(macd, signal_length)
hist = macd - signal
plot(hist, title="Histogram", style=plot.style_columns, color=(hist>=0 ? (hist[1] < hist ? col_grow_above : col_fall_above) : (hist[1] < hist ? col_grow_below : col_fall_below)))
plot(macd, title="MACD", color=#2962FF)
plot(signal, title="Signal", color=#FF6D00)
`;

export const SUPERTREND = `//@version=5
indicator("Supertrend", overlay = true, timeframe = "", timeframe_gaps = true)
atrPeriod = input.int(10, "ATR Length", minval = 1)
factor = input.float(3.0, "Factor", minval = 0.01, step = 0.01)
[supertrend, direction] = ta.supertrend(factor, atrPeriod)
supertrend := barstate.isfirst ? na : supertrend
upTrend = plot(direction < 0 ? supertrend : na, "Up Trend", color = color.green, style = plot.style_line)
downTrend = plot(direction < 0 ? na : supertrend, "Down Trend", color = color.red, style = plot.style_line)
bodyMiddle = plot(barstate.isfirst ? na : (open + close) / 2, "Body Middle", display = display.none)
fill(bodyMiddle, upTrend, color.new(color.green, 90), fillgaps = false)
fill(bodyMiddle, downTrend, color.new(color.red, 90), fillgaps = false)
alertcondition(direction[1] > direction, title = "Downtrend to Uptrend", message = "The Supertrend value switched from Downtrend to Uptrend ")
`;

export const BB_SQUEEZE = `//@version=6
indicator("Bollinger squeeze", overlay = false)
length = input.int(20, "Length")
mult = input.float(2.0, "StdDev")
lookback = input.int(50, "Squeeze lookback")
bbw = ta.bbw(close, length, mult)
minW = ta.lowest(bbw, lookback)
squeeze = bbw <= minW
plot(bbw, "Width", color = color.teal)
plotshape(squeeze, "Squeeze", shape.circle, location.bottom, color.orange)
`;

export const EMA_CROSS = `//@version=6
indicator("EMA cross", overlay = true)
fastLen = input.int(9, "Fast")
slowLen = input.int(21, "Slow")
fast = ta.ema(close, fastLen)
slow = ta.ema(close, slowLen)
plot(fast, "Fast", color.aqua)
plot(slow, "Slow", color.orange)
bull = ta.crossover(fast, slow)
bear = ta.crossunder(fast, slow)
plotshape(bull, "Buy", shape.triangleup, location.belowbar, color.green, size = size.small)
plotshape(bear, "Sell", shape.triangledown, location.abovebar, color.red, size = size.small)
alertcondition(bull or bear, "Cross", "EMA cross")
`;

export const PIVOTS = `//@version=5
indicator("Pivot points", overlay = true)
leftBars = input(5, "Left")
rightBars = input(5, "Right")
ph = ta.pivothigh(leftBars, rightBars)
pl = ta.pivotlow(leftBars, rightBars)
plotshape(ph, "PH", style = shape.labeldown, location = location.abovebar, color = color.red, text = "H", offset = -rightBars)
plotshape(pl, "PL", style = shape.labelup, location = location.belowbar, color = color.green, text = "L", offset = -rightBars)
`;

export const DMI = `//@version=5
indicator(title="Directional Movement Index", shorttitle="DMI", format=format.price, precision=4)
len = input.int(14, minval=1, title="DI Length")
lensig = input.int(14, title="ADX Smoothing", minval=1, maxval=50)
[diplus, diminus, adx] = ta.dmi(len, lensig)
plot(adx, color=#F50057, title="ADX")
plot(diplus, color=#2962FF, title="+DI")
plot(diminus, color=#FF6D00, title="-DI")
`;

export const USER_RSI = `//@version=6
indicator("Hand-written RSI")
myRsi(float src, simple int len) =>
    up = math.max(src - src[1], 0)
    down = math.max(src[1] - src, 0)
    rs = ta.rma(up, len) / ta.rma(down, len)
    100 - 100 / (1 + rs)
plot(myRsi(close, 14), "RSI")
`;

export const TUPLE_BANDS = `//@version=6
indicator("Tuple bands", overlay = true)
bands(src, len, k) =>
    basis = ta.sma(src, len)
    dev = k * ta.stdev(src, len)
    [basis, basis + dev, basis - dev]
[mid, up, lo] = bands(close, 20, 2)
p1 = plot(up, "Upper")
p2 = plot(lo, "Lower")
plot(mid, "Basis")
fill(p1, p2)
`;

export const MA_SWITCH = `//@version=6
indicator("MA selector", overlay = true)
maType = input.string("SMA", "Type", options = ["SMA", "EMA", "RMA", "WMA"])
len = input.int(14, "Length")
ma(src, l, t) =>
    switch t
        "SMA" => ta.sma(src, l)
        "EMA" => ta.ema(src, l)
        "RMA" => ta.rma(src, l)
        => ta.wma(src, l)
plot(ma(close, len, maType), "MA")
`;

export const FOR_SMA = `//@version=6
indicator("SMA by loop")
len = input.int(14, "Length")
sum = 0.0
for i = 0 to len - 1
    sum += close[i]
plot(sum / len, "Loop SMA")
`;

export const WHILE_STREAK = `//@version=6
indicator("Up streak")
n = 0
while n < 50 and close[n] > close[n + 1]
    n += 1
plot(n, "Streak")
`;

export const VAR_COUNTERS = `//@version=6
indicator("Counters")
var int ups = 0
var float hi = na
if close > open
    ups += 1
hi := na(hi) ? high : math.max(hi, high)
plot(ups, "Ups")
plot(hi, "Running high")
`;

export const HISTORY_CHAINS = `//@version=6
indicator("History chains")
m = close - close[5]
mm = m - m[3]
plot(mm, "MoM")
plot(ta.sma(close, 10)[2], "Lagged SMA")
plot(close[1][2], "close[3]")
`;

export const NA_HANDLING = `//@version=6
indicator("na handling")
src = bar_index % 7 == 3 ? na : close
var int gaps = 0
if na(src)
    gaps += 1
plot(fixnan(src), "Fixed")
plot(nz(src, -1), "Zeroed")
plot(gaps, "Gaps")
plot(src * 2, "Doubled")
`;

export const STOCH_RSI = `//@version=5
indicator(title="Stochastic RSI", shorttitle="Stoch RSI", format=format.price, precision=2)
smoothK = input.int(3, "K", minval=1)
smoothD = input.int(3, "D", minval=1)
lengthRSI = input.int(14, "RSI Length", minval=1)
lengthStoch = input.int(14, "Stochastic Length", minval=1)
src = input(close, title="RSI Source")
rsi1 = ta.rsi(src, lengthRSI)
k = ta.sma(ta.stoch(rsi1, rsi1, rsi1, lengthStoch), smoothK)
d = ta.sma(k, smoothD)
plot(k, "K", color=#2962FF)
plot(d, "D", color=#FF6D00)
h0 = hline(80, "Upper Band", color=#787B86)
h1 = hline(20, "Lower Band", color=#787B86)
fill(h0, h1, color=color.rgb(33, 150, 243, 90), title="Background")
`;
