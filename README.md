# hardware-monitor

![GitHub branch status](https://img.shields.io/github/checks-status/shm11C3/hardware-monitor/master)

![GitHub commits since tagged version](https://img.shields.io/github/commits-since/shm11C3/hardware-monitor/app-v0.1.0)



## Dashboard

![image](https://github.com/user-attachments/assets/9a2bf54f-d6e5-4c20-b0e4-f249fd5b8433)

## Usage Graph

![image](https://github.com/user-attachments/assets/b8fa7d67-a015-487f-aeb4-f43306d28f54)


## Development

### Requirements

- [Node.js 20+](https://nodejs.org/)
- [Rust](https://www.rust-lang.org/)
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites)

### Setup

1. Clone the repository:

   ```bash
   git clone https://github.com/shm11C3/hardware-monitor.git
   cd hardware-monitor
   ```

2. Install dependencies:

   ```bash
   npm ci
   ```

3. Run the app in development mode:

   ```bash
   npm run tauri dev
   ```

4. Build the app for production:

   ```bash
   npm run tauri build
   ```
