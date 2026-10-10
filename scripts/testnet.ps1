# Aleph Edge against the Binance USD-M futures TESTNET.
#
# A separate app identity (src-tauri/tauri.testnet.conf.json), so this run has
# its own vault, database, bots and positions: the real install is untouched.
# The live order path is compiled in (--features live) and every order goes to
# https://testnet.binancefuture.com. Testnet balances are not real money.
#
#   powershell -ExecutionPolicy Bypass -File scripts\testnet.ps1

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$env:ALEPH_EDGE_BINANCE_FUTURES_BASE = "https://testnet.binancefuture.com"
npm run tauri dev -- --features live --config src-tauri/tauri.testnet.conf.json
