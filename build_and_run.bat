@echo off
cargo build --release --package rh-terminal
if %errorlevel% neq 0 exit /b %errorlevel%
target\release\rogue-hunter.exe
