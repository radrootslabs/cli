use crate::domain::runtime::{
    AccountListView, AccountNewView, AccountSummaryView, AccountUseView, AccountWhoamiView,
    CommandDisposition, CommandOutput, CommandView, IdentityPublicView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{
    AccountCreateMode, AccountRecordView, SHARED_ACCOUNT_STORE_SOURCE,
    create_or_migrate_selected_account, resolve_account, select_account, snapshot,
};
use crate::runtime::config::RuntimeConfig;

pub fn init(config: &RuntimeConfig) -> Result<AccountNewView, RuntimeError> {
    let result = create_or_migrate_selected_account(config)?;
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
    let view = match resolve_account(config)? {
        Some(account) => AccountWhoamiView {
            state: "ready".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            reason: None,
            public_identity: Some(IdentityPublicView::from_public_identity(
                &account.record.public_identity,
            )),
            account: Some(account_summary(&account)),
        },
        None => AccountWhoamiView {
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            reason: Some(format!(
                "no local account is selected in {}",
                config.account.store_path.display()
            )),
            account: None,
            public_identity: None,
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
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::AccountWhoami(view))
        }
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
        vec!["radroots account new".to_owned()]
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
        state: "active".to_owned(),
        source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
        active_account_id: account.record.account_id.to_string(),
        account: account_summary(&account),
    })
}

fn account_summary(account: &AccountRecordView) -> AccountSummaryView {
    AccountSummaryView::from_account_record(&account.record, account.signer, account.selected)
}
