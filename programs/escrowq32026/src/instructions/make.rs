use anchor_lang::prelude::*;

use crate::{Escrow, ESCROW_SEED};
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

// Accounts required to open a new escrow: the maker deposits Mint A into a
// vault owned by the escrow PDA, and specifies how much Mint B they want back.
#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Make<'info> {
    // The user creating the escrow — pays for account creation and signs the deposit
    #[account(mut)]
    pub maker: Signer<'info>,

    // Mint of the token the maker is depositing
    #[account(
        mint::token_program = token_program
    )]
    pub mint_a: InterfaceAccount<'info, Mint>,

    // Mint of the token the maker wants to receive in exchange
    #[account(
        mint::token_program = token_program
    )]
    pub mint_b: InterfaceAccount<'info, Mint>,

    // Maker's existing token account for Mint A — source of the deposit
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>,

    // PDA that stores the escrow's terms (mints, amounts, expiration).
    // Seeded by a constant + the maker's pubkey + a caller-supplied `seed`,
    // so one maker can have multiple concurrent escrows.
    #[account(
        init,
        payer = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), seed.to_le_bytes().as_ref()],
        space = Escrow::DISCRIMINATOR.len() + Escrow::INIT_SPACE,
        bump
    )]
    pub escrow: Account<'info, Escrow>,

    // Vault token account, owned by the escrow PDA — holds the deposited
    // Mint A until a taker takes the trade or the maker refunds it
    #[account(
        init,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Make<'info> {
    // Writes the escrow terms into the newly created escrow account
    pub fn init_escrow(
        &mut self,
        seed: u64,
        receive: u64,
        bumps: &MakeBumps,
        expiration: i64,
    ) -> Result<()> {
        self.escrow.set_inner(Escrow {
            seed,
            maker: self.maker.key(),
            mint_a: self.mint_a.key(),
            mint_b: self.mint_b.key(),
            receive: receive,     // amount of Mint B the maker wants in return
            bump: bumps.escrow,   // store the PDA bump so it can re-derive/sign later
            expiration: expiration,
        });
        Ok(())
    }

    // Moves the maker's deposit from their wallet into the vault
    pub fn deposit(&mut self, deposit: u64) -> Result<()> {
        let transfer_accounts = TransferChecked {
            from: self.maker_ata_a.to_account_info(),
            mint: self.mint_a.to_account_info(),
            to: self.vault.to_account_info(),
            authority: self.maker.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(self.token_program.key(), transfer_accounts);

        transfer_checked(cpi_ctx, deposit, self.mint_a.decimals)
    }
}