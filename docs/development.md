# Development

How the script studio is run while it is being changed, without touching the live records.

## The copy of the data root

- **Where:** a workbench folder outside the data root it copies, named with `--to`, by default the studio's data root
  with `-workbench` after its name.
- **What it holds:** the source manifest, the download records and the research index, copied; `studio.db`, copied
  through SQLite (`VACUUM INTO`), which reads through the write-ahead log so every committed review is in the copy;
  and `media/`, `subtitles/` and `asr/`, linked to the live ones, which the server only reads.
- **Making and refreshing it:** `bun studio/scripts/dev-data.ts`. A refresh replaces the copy's records and database
  with the live ones and lays the links again; `--from` and `--to` name another source and copy.

## The bench

- **`bun studio/scripts/dev.ts`** runs two processes and makes the copy first where there is none:
  - **the server**, the `script-studio` binary built and started by Cargo against the copy (`BMO_SCRIPT_DATA`), on
    its own loopback port, signing people in against the web unit the development settings name;
  - **the screen**, Nuxt's dev server under `/bmo-script/` with hot reload, passing `api`, `media` and `subtitle`
    through to the server and keeping the request's Host header, which the server matches a write's Origin against.
- **When either process ends the other is ended too.** A job that needs the bench starts it in the background, reads
  its output from a file, and takes it down before it ends. Its ports are fixed, so one bench runs at a time.
- **A login** is an account of the web unit the development settings name.
