create table "email" (
    "id" uuid not null default uuidv7(),
    "from" text not null,
    "to" text not null,
    "subject" text not null,
    "body" text not null,
    "sent_at" timestamptz,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "version" int8 not null default 0,
    constraint "email_pkey" primary key ("id")
);
