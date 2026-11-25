create table "tenant" (
    "id" uuid not null default uuidv7(),
    "name" text not null,
    constraint "tenant_pkey" primary key ("id")
);

create table "user" (
    "id" uuid not null default uuidv7(),
    "email" text not null unique,
    "email_verified_at" timestamptz,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "deleted_at" timestamptz,
    "version" bigint not null default 0,
    constraint "user_pkey" primary key ("id")
);

create table "user_session" (
    "id" uuid not null default uuidv7(),
    "user_id" uuid not null references "user"("id"),
    "token_hash" text not null unique,
    "expires_at" timestamptz not null,
    "revoked_at" timestamptz,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "version" bigint not null default 0,
    constraint "user_session_pkey" primary key ("id")
);
