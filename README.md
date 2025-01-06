# Gitup - Your Open Source Backup Solution

Gitup is a powerful and flexible open-source backup utility that allows you to securely store full or incremental
backups of your data using Git repositories on GitHub or GitLab. Designed for both simplicity and efficiency, Gitup
provides an easy-to-use graphical interface (GUI) and a robust command-line daemon that operates seamlessly in the
background to handle your backup needs.

---

## Key Features

### 1. **Versioning Power of Git**

Gitup leverages Git's versioning capabilities to ensure your backups are safe, secure, and easily recoverable. This
eliminates the risk of data loss while providing a structured way to track and manage changes to your files over time.

### 2. **Backup Modes**

Gitup supports two primary modes of operation:

- **Full Backup**: Create a complete snapshot of your data at a specific point in time.
- **Incremental Backup**: Save only the changes made since the last backup, reducing storage requirements and speeding
  up the backup process.

### 3. **Configurable Backup Options**

Gitup offers advanced configuration options to tailor your backup strategy to your needs:

- **Folder-Specific Branching**: Backup each folder in a separate Git branch for independent versioning and simplified
  management.
- **File Splitting**: Automatically split files that exceed a predefined size to optimize storage and ensure
  compatibility with repository limits.
- **Multipart Backups**: Record each file independently within the repository to facilitate granular backup and recovery
  processes.

### 4. **Dual Interfaces**

- **Graphical User Interface (GUI)**: An intuitive interface for users who prefer visual interactions.
- **Command-Line Daemon**: A lightweight and efficient background process that executes backups without requiring manual
  intervention.

---

## Why Choose Gitup?

- **Open Source**: Fully transparent and community-driven, with the freedom to inspect, modify, and contribute to the
  code.
- **Cross-Platform**: Designed to work seamlessly across different operating systems.
- **Secure**: Utilizes Git's secure protocol and encryption capabilities to safeguard your data.
- **Customizable**: Highly configurable to adapt to various use cases and storage requirements.
- **Efficient**: Combines the robustness of Git with intelligent backup mechanisms to minimize resource usage.

---

## Getting Started

### Prerequisites

- A GitHub or GitLab account with access to create repositories.
- An access token with the "Content" (read-write) permission, which you can generate in your account settings.

### Installation

1. Download and run the installer, you can find it in the [releases page]().

### Configuration

1. Launch the GUI or edit the configuration file (`config.ini`) to specify your backup preferences, including repository
   URL, backup mode, and file splitting options.
2. Set up the daemon to run in the background:
   ```bash
   gitup-daemon start
   ```

---

## Contribution

We welcome contributions from the community to enhance Gitup! If you'd like to contribute:

1. Fork the repository.
2. Create a new branch for your feature or bugfix:
   ```bash
   git checkout -b feature-name
   ```
3. Commit your changes and push to your branch.
4. Submit a pull request.

---

## License

Gitup is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). See the `LICENSE` file
for more details.

---

## Support

If you encounter issues or have questions, feel free to open an issue on GitHub or reach out to the community via the
discussions board.

---

## Roadmap

Planned features for future releases:

- Support for additional Git hosting platforms.
- Enhanced encryption and compression options.
- Cloud storage integration for non-Git backups.
- Scheduling capabilities for automated backups.

---

Gitup: The future of secure, efficient, and open-source backups. Start versioning your data today!

