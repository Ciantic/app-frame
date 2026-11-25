create table "customer" (
    "id" uuid not null default uuidv7(),
    "tenant_id" uuid not null,
    "name" text not null,
    "email" text not null,
    "address" text not null,
    "business_id" text not null,
    "e_invoice_address" text,
    "e_invoice_operator" text,
    "language" text,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "version" bigint not null default 0,
    constraint "customer_pkey" primary key ("id"),
    constraint "customer_tenant_id_id_key" unique ("tenant_id", "id")
);

create table "invoice" (
    "id" uuid not null default uuidv7(),
    "tenant_id" uuid not null,
    "number" text unique,
    "customer_id" uuid,
    "seller_id" uuid,
    "language" text,
    "issue_date" date,
    "due_date" date,
    "net_amount" numeric(14,2) not null default 0,
    "tax_amount" numeric(14,2) not null default 0,
    "total_amount" numeric(14,2) not null default 0,
    "notes" text,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "deleted_at" timestamptz,
    "version" bigint not null default 0,
    constraint "invoice_pkey" primary key ("id"),
    constraint "invoice_tenant_id_id_key" unique ("tenant_id", "id"),
    constraint "invoice_customer_id_fkey" foreign key ("tenant_id", "customer_id")
        references "customer" ("tenant_id", "id")
);

create index "invoice_customer_id_idx" on "invoice" ("customer_id");

create table "invoice_row" (
    "id" uuid not null default uuidv7(),
    "tenant_id" uuid not null,
    "invoice_id" uuid not null,
    "description" text,
    "quantity" numeric(12,3),
    "unit" text,
    "unit_price" numeric(14,2),
    "tax_rate" numeric(5,4),
    "net_amount" numeric(14,2) not null default 0,
    "tax_amount" numeric(14,2) not null default 0,
    "total_amount" numeric(14,2) not null default 0,
    "created_at" timestamptz not null default now(),
    "updated_at" timestamptz not null default now(),
    "version" bigint not null default 0,
    constraint "invoice_row_pkey" primary key ("id"),
    constraint "invoice_row_invoice_id_fkey" foreign key ("tenant_id", "invoice_id")
        references "invoice" ("tenant_id", "id") on delete cascade
);

create index "invoice_row_invoice_id_idx" on "invoice_row" ("invoice_id");

create function "invoice_row_amounts"() returns trigger language plpgsql as $$
begin
    new."net_amount" := round(coalesce(new."quantity", 0) * coalesce(new."unit_price", 0), 2);
    new."tax_amount" := round(new."net_amount" * coalesce(new."tax_rate", 0), 2);
    new."total_amount" := new."net_amount" + new."tax_amount";
    return new;
end;
$$;

create trigger "invoice_row_amounts"
before insert or update on "invoice_row"
for each row execute function "invoice_row_amounts"();

create function "invoice_totals"() returns trigger language plpgsql as $$
begin
    new."net_amount" := coalesce((
        select sum("net_amount") from "invoice_row"
        where "invoice_id" = new."id" and "tenant_id" = new."tenant_id"
    ), 0);
    new."tax_amount" := coalesce((
        select sum("tax_amount") from "invoice_row"
        where "invoice_id" = new."id" and "tenant_id" = new."tenant_id"
    ), 0);
    new."total_amount" := new."net_amount" + new."tax_amount";
    return new;
end;
$$;

create trigger "invoice_totals"
before insert or update on "invoice"
for each row execute function "invoice_totals"();

create function "invoice_row_totals"() returns trigger language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        update "invoice" set "updated_at" = now(), "version" = "version" + 1
        where "id" = old."invoice_id" and "tenant_id" = old."tenant_id";
        return old;
    end if;

    update "invoice" set "updated_at" = now(), "version" = "version" + 1
    where "id" = new."invoice_id" and "tenant_id" = new."tenant_id";

    if tg_op = 'UPDATE'
        and (old."invoice_id", old."tenant_id") is distinct from (new."invoice_id", new."tenant_id")
    then
        update "invoice" set "updated_at" = now(), "version" = "version" + 1
        where "id" = old."invoice_id" and "tenant_id" = old."tenant_id";
    end if;

    return new;
end;
$$;

create trigger "invoice_row_totals"
after insert or update or delete on "invoice_row"
for each row execute function "invoice_row_totals"();
