@echo off
REM Display a message to the user
echo Setting up Verso on Windows...

REM Install Scoop (if not installed)
where scoop >nul 2>&1
if %errorlevel% neq 0 (
    echo Scoop not found. Installing Scoop...
    powershell -Command "Set-ExecutionPolicy RemoteSigned -scope CurrentUser"
    powershell -Command "iwr -useb get.scoop.sh | iex"
)

REM Install required tools via Scoop
scoop install git python llvm cmake curl

REM Install Python dependencies
pip install mako

REM Build and run the project using Cargo
cargo run

REM Display completion message
echo Setup complete. Press any key to exit.
pause
