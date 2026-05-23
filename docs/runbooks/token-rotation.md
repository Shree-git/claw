# Runbook: Token Rotation

## Purpose

Rotate bearer tokens used for daemon auth and sync clients.

## Rotation model

Claw validates a single bearer token value at daemon runtime. Rotation is a coordinated update:

1. Write new token to the profile used by daemon and clients.
2. Restart daemon with the updated profile/token.
3. Validate client operations.

## Procedure

1. Generate new strong random token in your secret manager.
2. Update daemon host profile:

```bash
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --profile default
```

3. Update client profile(s) that use `--token-profile default`:

```bash
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --profile default
```

4. Restart daemon.
5. Validate with a sync operation:

```bash
claw sync pull --remote origin --ref-name heads/main
```

## Verification

- New token succeeds.
- Old token fails with `invalid bearer token`.
- Daemon logs show stable authenticated traffic.

## Rollback

If rotation breaks connectivity:

1. Re-set previous known-good token in affected profiles.
2. Restart daemon.
3. Re-run sync validation.

## Notes

- Auth profiles are stored in `~/.claw/auth.toml` with encrypted token fields
  and private file permissions.
- Keep profile names consistent across automation and runbooks.
