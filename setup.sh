#!/bin/bash
# Display a message to the user
echo "Setting up Verso on Unix-based systems..."

# Function to install packages on Linux and macOS
install_packages() {
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        echo "Detected Linux. Installing dependencies..."
        if [ -x "$(command -v apt-get)" ]; then
            sudo apt-get update
            sudo apt-get install -y git python3-pip llvm cmake curl
        elif [ -x "$(command -v yum)" ]; then
            sudo yum install -y git python3-pip llvm cmake curl
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        echo "Detected macOS. Installing dependencies via Homebrew..."
        if ! command -v brew &> /dev/null; then
            echo "Homebrew not found. Installing Homebrew..."
            /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        fi
        brew install cmake pkg-config harfbuzz
    else
        echo "Unsupported OS: $OSTYPE"
        exit 1
    fi
}

# Install necessary packages
install_packages

# Install Python dependencies
pip3 install mako

# Build and run the project using Cargo
cargo run

# Display completion message
echo "Setup complete."
