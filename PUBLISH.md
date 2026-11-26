# 发布

```sh
pnpm version:set 0.3.15
pnpm version:check
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(release): 发布 0.3.15"
git tag v0.3.15
```

确认提交与 Tag 后，手动推送到 GitHub。以下示例使用本地已配置的 `github` 远程名，其他克隆需替换为实际 GitHub 远程名（通常为 `origin`）：

```sh
git push github master
git push github v0.3.12
```