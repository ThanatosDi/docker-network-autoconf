FROM python:3.14-slim

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

WORKDIR /app

# 先安裝依賴，pyproject.toml / uv.lock 未變動時可沿用此 layer cache
COPY pyproject.toml uv.lock .python-version ./
RUN uv sync --locked --no-install-project

COPY . /app
RUN uv sync --locked

CMD ["uv", "run", "main.py"]
