# Gitup - Your Open Source Backup Solution

Gitup is a powerful and flexible open-source backup utility designed to optimize free or low-cost storage options across
various providers, including GitHub, GitLab, TeraBox, MEGA.nz, Google Drive, and more. With both a user-friendly
graphical interface (GUI) and a robust command-line daemon, Gitup enables secure, efficient, and customizable backups to
help you manage your data effectively.

## Key Features

### 1. **Optimized Storage Management**

Gitup leverages the storage capabilities of multiple platforms to provide cost-effective solutions for your backup
needs. It includes intelligent management to maximize the use of free storage tiers while adhering to the fair usage
policies of each provider.

### 2. **Backup Modes**

Gitup supports two primary modes of operation:

- **Full Backup**: Create a complete snapshot of your data at a specific point in time.
- **Incremental Backup**: Save only the changes made since the last backup, reducing storage requirements and speeding
  up the process.

### 3. **Configurable Backup Options**

Gitup offers advanced configuration options to tailor your backup strategy to your needs:

- **Provider-Specific Configurations**: Customize backup settings for each supported storage provider to optimize
  compatibility and performance.
- **File Splitting**: Automatically split files that exceed a predefined size to ensure compliance with provider
  limitations.
- **Multipart Backups**: Record each file independently within the storage repository for granular backup and recovery
  processes.
- **Compressed Archives**: Create compressed backups to reduce storage size and enhance efficiency.
- **Encryption**: Encrypt your data to ensure higher security and protect sensitive information.

### 4. **Dual Interfaces**

- **Graphical User Interface (GUI)**: An intuitive interface for users who prefer visual interactions. The GUI allows
  you to configure your backup preferences, which are saved internally in a JSON configuration file.
- **Command-Line Daemon**: A lightweight and efficient background process that executes backups without requiring manual
  intervention.

## Disclaimer on Usage

**Important:** While Gitup supports various storage providers, including GitHub and GitLab, it is your responsibility to
ensure compliance with their terms of service. Many platforms prohibit using their repositories or services as
general-purpose storage solutions. Gitup is designed to assist with fair and optimized usage within these constraints.
Misuse of this tool may result in the suspension or termination of your account by the provider. Always review the
acceptable use policies of your chosen storage platforms before proceeding.

## Why Choose Gitup?

- **Open Source**: Fully transparent and community-driven, with the freedom to inspect, modify, and contribute to the
  code.
- **Cross-Platform**: Designed to work seamlessly across different operating systems.
- **Secure**: Incorporates encryption and authentication mechanisms to safeguard your data.
- **Customizable**: Highly configurable to adapt to various use cases and storage requirements.
- **Efficient**: Intelligent backup mechanisms minimize resource usage while maximizing storage potential.

## Getting Started

### Prerequisites

- Accounts with supported storage providers (e.g., GitHub, Google Drive, MEGA.nz).
- Appropriate credentials or tokens for accessing these platforms.

### Installation

1. Download and run the installer. You can find it on the [releases page](https://github.com/ebalo55/gitup/releases).

### Configuration

1. Launch the GUI to specify your backup preferences, including storage providers, backup mode, encryption, and file
   splitting options. The GUI will save these settings internally to a JSON configuration file.
2. Alternatively, edit the JSON configuration file manually if you prefer direct control over your settings.
3. Set up the daemon to run in the background:
   ```bash
   gitup-daemon start
   ```

## Contribution

We welcome contributions from the community to enhance Gitup! If you'd like to contribute:

1. Fork the repository.
2. Create a new branch for your feature or bugfix:
   ```bash
   git checkout -b feature-name
   ```
3. Commit your changes and push to your branch.
4. Submit a pull request.

## License

Gitup is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). See the `LICENSE` file
for more details.

## Support

If you encounter issues or have questions, feel free to open an issue on GitHub or reach out to the community via the
discussions board.

If you like Gitup, please consider giving us a ⭐star on GitHub
and [sponsoring the project](https://github.com/sponsors/ebalo55) to support its development.

## Roadmap

Planned features for future releases:

- Expanded support for additional storage providers.
- Enhanced encryption and compression options.
- Scheduling capabilities for automated backups.
- Improved monitoring and reporting tools.

---

Gitup: The future of cost-effective, secure, and open-source backups. Start managing your data efficiently today!

