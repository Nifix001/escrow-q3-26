use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{state::Escrow, ESCROW_SEED, error::ErrorCode};

// Accounts required for a taker to fulfill an escrow: pay the maker in
// Mint B, then receive the maker's original Mint A deposit from the vault.
#[derive(Accounts)]
pub struct Take<'info> {
    // The user accepting the trade — pays for any new ATAs and signs the payment
    #[account(mut)]
    pub taker: Signer<'info>,

    // Must be marked `mut`: the escrow's `close = maker` constraint and the
    // vault's CloseAccount CPI both send lamports into this account, so it
    // needs to be writable at the top level or the CPI fails with a
    // "privilege escalation" error
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    #[account(
        mint::token_program = token_program
    )]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mint::token_program = token_program
    )]
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    // Taker's token account for Mint A — created on the fly if it doesn't
    // exist yet, since this is where they'll receive the escrowed deposit
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint= mint_a,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    // Taker's existing token account for Mint B — source of their payment.
    // Not init_if_needed: the taker must already hold Mint B before taking.
    #[account(
        mut,
        associated_token::mint= mint_b,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Maker's token account for Mint B — created on the fly if needed,
    // since this is where the taker's payment lands
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint= mint_b,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // The escrow being fulfilled. has_one constraints tie it to mint_a and
    // the real maker; close = maker refunds the escrow's rent once done.
    #[account(
        mut,
        close = maker,
        has_one = mint_a,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    // Vault holding the maker's original Mint A deposit
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Take<'info> {
    // Pays the maker: transfers `escrow.receive` amount of Mint B from
    // taker to maker. Rejected once the escrow has expired.
    pub fn deposit(&mut self) -> Result<()> {
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;
        require!(
            self.escrow.expiration > current_time,
            ErrorCode::EscrowExpired
        );

        let transfer_accounts = TransferChecked {
            from: self.taker_ata_b.to_account_info(),
            mint: self.mint_b.to_account_info(),
            to: self.maker_ata_b.to_account_info(),
            authority: self.taker.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(self.token_program.key(), transfer_accounts);

        transfer_checked(cpi_ctx, self.escrow.receive, self.mint_b.decimals)?;
        Ok(())
    }

    // Releases the vault's Mint A to the taker, then closes the (now empty) vault
    pub fn withdraw_and_close_vault(&mut self) -> Result<()> {
        // Seeds let the escrow PDA sign on its own behalf, since it (not a
        // real wallet) is the authority over the vault
        let seeds = &[
            ESCROW_SEED,
            self.maker.key.as_ref(),
            &self.escrow.seed.to_le_bytes()[..],
            &[self.escrow.bump],
        ];

        let signer_seeds = &[&seeds[..]];

        // Send the full vault balance to the taker
        let transfer_accounts = TransferChecked {
            from: self.vault.to_account_info(),
            mint: self.mint_a.to_account_info(),
            to: self.taker_ata_a.to_account_info(),
            authority: self.escrow.to_account_info(),
        };

        let cpi_ctx =
            CpiContext::new_with_signer(self.token_program.key(), transfer_accounts, signer_seeds);

        transfer_checked(cpi_ctx, self.vault.amount, self.mint_a.decimals)?;

        // Vault is empty — close it and reclaim its rent to the maker
        let close_accounts = CloseAccount {
            account: self.vault.to_account_info(),
            destination: self.maker.to_account_info(),
            authority: self.escrow.to_account_info(),
        };

        let close_cpi_ctx =
            CpiContext::new_with_signer(self.token_program.key(), close_accounts, signer_seeds);

        close_account(close_cpi_ctx)?;

        Ok(())
    }
}