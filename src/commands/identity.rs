use crate::cli::AccountImportArgs;
use crate::domain::runtime::{
    AccountClearDefaultView, AccountImportView, AccountListView, AccountNewView, AccountRemoveView,
    AccountSummaryView, AccountUseView, AccountWhoamiView, CommandDisposition, CommandOutput,
    CommandView, IdentityPublicView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{
    AccountCreateMode, AccountRecordView, SHARED_ACCOUNT_STORE_SOURCE, account_resolution_view,
    account_summary_view, clear_default_account, create_or_migrate_default_account,
    import_public_identity, remove_account as remove_stored_account, resolve_account_resolution,
    select_account, snapshot, unresolved_account_reason,
};
use crate::runtime::config::RuntimeConfig;

pub fn init(config: &RuntimeConfig) -> Result<AccountNewView, RuntimeError> {
    let result = create_or_migrate_default_account(config)?;
    let account = account_summary(&result.account);
    Ok(AccountNewView {
        state: match result.mode {
            AccountCreateMode::Created => "created".to_owned(),
            AccountCreateMode::Migrated => "migrated".to_owned(),
        },
        source: match result.mode {
            AccountCreateMode::Created => SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            AccountCreateMode::Migrated => "legacy shared identity import · local first".to_owned(),
        },
        public_identity: IdentityPublicView::from_public_identity(
            &result.account.record.public_identity,
        ),
        account,
        actions: vec![
            "radroots account whoami".to_owned(),
            "radroots account ls".to_owned(),
        ],
    })
}

pub fn show(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let resolution = resolve_account_resolution(config)?;
    let snapshot = snapshot(config)?;
    let view = match resolution.resolved_account.as_ref() {
        Some(account) => AccountWhoamiView {
            state: "ready".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            reason: None,
            account_resolution: account_resolution_view(&resolution),
            public_identity: Some(IdentityPublicView::from_public_identity(
                &account.record.public_identity,
            )),
            actions: Vec::new(),
        },
        None => AccountWhoamiView {
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            reason: Some(unresolved_account_reason(config)?),
            account_resolution: account_resolution_view(&resolution),
            public_identity: None,
            actions: unresolved_account_actions(snapshot.accounts.is_empty()),
        },
    };

    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::AccountWhoami(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::AccountWhoami(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::AccountWhoami(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::AccountWhoami(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::AccountWhoami(view))
        }
    })
}

pub fn import(
    config: &RuntimeConfig,
    args: &AccountImportArgs,
) -> Result<AccountImportView, RuntimeError> {
    let account = import_public_identity(config, args.path.as_path(), args.default)?;
    let account_view = account_summary(&account);
    Ok(AccountImportView {
        state: "imported".to_owned(),
        source: "shared account store · watch-only import".to_owned(),
        public_identity: IdentityPublicView::from_public_identity(&account.record.public_identity),
        actions: if account.is_default {
            vec![
                "radroots account view".to_owned(),
                "radroots account list".to_owned(),
            ]
        } else {
            vec![
                "radroots account list".to_owned(),
                "radroots account select <selector>".to_owned(),
            ]
        },
        account: account_view,
    })
}

pub fn list(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let snapshot = snapshot(config)?;
    let accounts = snapshot
        .accounts
        .iter()
        .map(account_summary)
        .collect::<Vec<_>>();
    let actions = if accounts.is_empty() {
        vec![
            "radroots account create".to_owned(),
            "radroots account import <path>".to_owned(),
        ]
    } else {
        Vec::new()
    };
    Ok(CommandOutput::success(CommandView::AccountList(
        AccountListView {
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            count: accounts.len(),
            accounts,
            actions,
        },
    )))
}

pub fn use_account(config: &RuntimeConfig, selector: &str) -> Result<AccountUseView, RuntimeError> {
    let account = select_account(config, selector)?;
    Ok(AccountUseView {
        state: "default".to_owned(),
        source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
        default_account_id: account.record.account_id.to_string(),
        account: account_summary(&account),
    })
}

pub fn clear_default(config: &RuntimeConfig) -> Result<AccountClearDefaultView, RuntimeError> {
    let result = clear_default_account(config)?;
    let cleared_account = result.cleared_account.as_ref().map(account_summary);
    Ok(AccountClearDefaultView {
        state: if cleared_account.is_some() {
            "cleared".to_owned()
        } else {
            "already_clear".to_owned()
        },
        source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
        actions: follow_up_account_actions(result.remaining_account_count),
        cleared_account,
        remaining_account_count: result.remaining_account_count,
    })
}

pub fn remove(config: &RuntimeConfig, selector: &str) -> Result<AccountRemoveView, RuntimeError> {
    let result = remove_stored_account(config, selector)?;
    Ok(AccountRemoveView {
        state: "removed".to_owned(),
        source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
        removed_account: account_summary(&result.removed_account),
        default_cleared: result.default_cleared,
        remaining_account_count: result.remaining_account_count,
        actions: if result.default_cleared {
            follow_up_account_actions(result.remaining_account_count)
        } else {
            Vec::new()
        },
    })
}

fn account_summary(account: &AccountRecordView) -> AccountSummaryView {
    account_summary_view(account)
}

fn unresolved_account_actions(has_accounts: bool) -> Vec<String> {
    if has_accounts {
        vec![
            "radroots account list".to_owned(),
            "radroots account select <selector>".to_owned(),
        ]
    } else {
        vec![
            "radroots account create".to_owned(),
            "radroots account import <path>".to_owned(),
        ]
    }
}

fn follow_up_account_actions(remaining_account_count: usize) -> Vec<String> {
    if remaining_account_count == 0 {
        vec![
            "radroots account create".to_owned(),
            "radroots account import <path>".to_owned(),
        ]
    } else {
        vec![
            "radroots account list".to_owned(),
            "radroots account select <selector>".to_owned(),
        ]
    }
}
