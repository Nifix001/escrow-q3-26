use {
    anchor_lang::{
        AccountDeserialize, InstructionData, ToAccountMetas, prelude::msg, solana_program::{instruction::Instruction, program_pack::Pack}, system_program::ID as SYSTEM_PROGRAM_ID,
    }, anchor_spl::{
        associated_token::{self, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
        token::spl_token,
    }, escrowq32026::Clock, litesvm::LiteSVM, litesvm_token::{
        CreateAssociatedTokenAccount, CreateMint, MintTo, spl_token::ID as TOKEN_PROGRAM_ID,
    }, solana_keypair::Keypair, solana_message::Message, solana_pubkey::Pubkey, solana_signer::Signer, solana_transaction::Transaction,
};

// Setup function to initialize LiteSVM and create a payer keypair
fn setup() -> (LiteSVM, Keypair) {
    let program_id = escrowq32026::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/escrowq32026.so"
    ));
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    // Return the LiteSVM instance and payer keypair
    (svm, payer)
}

#[test]
fn test_make_and_refund() {
    // Setup the test environment by initializing LiteSVM and creating a payer keypair
    let (mut program, payer) = setup();

    // Get the maker's public key from the payer keypair
    let maker = payer.pubkey();

    // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
    // This done using litesvm-token's CreateMint utility which creates the mint in the LiteSVM environment
    let mint_a = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    msg!("Mint A: {}\n", mint_a);

    let mint_b = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    msg!("Mint B: {}\n", mint_b);

    // Create the maker's associated token account for Mint A
    // This is done using litesvm-token's CreateAssociatedTokenAccount utility
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
        .owner(&maker)
        .send()
        .unwrap();
    msg!("Maker ATA A: {}\n", maker_ata_a);

    // Derive the PDA for the escrow account using the maker's public key and a seed value
    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker.as_ref(), &123u64.to_le_bytes()],
        &escrowq32026::id(),
    )
    .0;
    msg!("Escrow PDA: {}\n", escrow);

    // Derive the PDA for the vault associated token account using the escrow PDA and Mint A
    let vault = associated_token::get_associated_token_address(&escrow, &mint_a);
    msg!("Vault PDA: {}\n", vault);

    // Mint 1,000 tokens (with 6 decimal places) of Mint A to the maker's associated token account
    MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000_000_000)
        .send()
        .unwrap();

    // Create the "Make" instruction to deposit tokens into the escrow
    let make_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Make {
            maker: maker,
            mint_a: mint_a,
            mint_b: mint_b,
            maker_ata_a: maker_ata_a,
            escrow: escrow,
            vault: vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Make {
            deposit: 10_000_000,
            seed: 123u64,
            receive: 10_000_000,
            expiration: 17780206209,
        }
        .data(),
    };

    // Create and send the transaction containing the "Make" instruction
    let message = Message::new(&[make_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();

    let transaction = Transaction::new(&[&payer], message, recent_blockhash);

    // Send the transaction and capture the result
    let tx = program.send_transaction(transaction).unwrap();

    // Log transaction details
    msg!("\n\nMake transaction sucessfull");
    msg!("CUs Consumed: {}", tx.compute_units_consumed);
    msg!("Tx Signature: {}", tx.signature);

    // Verify the vault account and escrow account data after the "Make" instruction
    let vault_account = program.get_account(&vault).unwrap();
    let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
    assert_eq!(vault_data.amount, 10_000_000);
    assert_eq!(vault_data.owner, escrow);
    assert_eq!(vault_data.mint, mint_a);

    let escrow_account = program.get_account(&escrow).unwrap();
    let escrow_data =
        escrowq32026::state::Escrow::try_deserialize(&mut escrow_account.data.as_ref()).unwrap();
    assert_eq!(escrow_data.seed, 123u64);
    assert_eq!(escrow_data.maker, maker);
    assert_eq!(escrow_data.mint_a, mint_a);
    assert_eq!(escrow_data.mint_b, mint_b);
    assert_eq!(escrow_data.receive, 10_000_000);

    // Create the "Refund" instruction to refund tokens back to the maker
    let refund_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Refund {
            maker: maker,
            mint_a: mint_a,
            maker_ata_a: maker_ata_a,
            escrow: escrow,
            vault: vault,
            token_program: TOKEN_PROGRAM_ID,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Refund {}.data(),
    };

    // Create and send the transaction containing the "Refund" instruction
    let message = Message::new(&[refund_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();

    let transaction = Transaction::new(&[&payer], message, recent_blockhash);

    // Send the transaction and capture the result
    let tx = program.send_transaction(transaction).unwrap();

    // Log transaction details
    msg!("\n\nRefund transaction sucessful");
    msg!("CUs Consumed: {}", tx.compute_units_consumed);
    msg!("Tx Signature: {}", tx.signature);
    assert!(program.get_account(&escrow).is_none());
    assert!(program.get_account(&vault).is_none());
}

#[test]
fn test_make_and_take() {
    // Fresh LiteSVM instance + funded payer (acts as the maker)
    let (mut program, payer) = setup();
    let maker = payer.pubkey();

    // Taker needs its own keypair since it must sign the "Take" instruction
    let taker_kp = Keypair::new();
    let taker = taker_kp.pubkey();
    // Airdrop taker some SOL to pay for rent on init_if_needed accounts (taker_ata_a, maker_ata_b)
    program.airdrop(&taker, 1_000_000_000).unwrap();

    // Create Mint A (the token the maker deposits) and Mint B (the token the maker wants in return)
    let mint_a = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();

    let mint_b = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();

    // Maker's ATA for Mint A — this is what funds the vault during "Make"
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
        .owner(&maker)
        .send()
        .unwrap();

    // Taker's ATA for Mint B must be created up front and pre-funded,
    // because `taker_ata_b` in the Take accounts struct is `mut`, not `init_if_needed`
    let taker_ata_b = CreateAssociatedTokenAccount::new(&mut program, &taker_kp, &mint_b)
        .owner(&taker)
        .send()
        .unwrap();

    // Give the taker some Mint B tokens to pay the maker with
    MintTo::new(&mut program, &payer, &mint_b, &taker_ata_b, 10_000_000)
        .send()
        .unwrap();

    // Derive the escrow PDA (must match the seeds used in the program: b"escrow", maker, seed)
    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker.as_ref(), &124u64.to_le_bytes()],
        &escrowq32026::id(),
    )
    .0;

    // Derive the vault ATA — owned by the escrow PDA, holds Mint A while the trade is pending
    let vault = associated_token::get_associated_token_address(&escrow, &mint_a);

    // Fund the maker with Mint A so there's something to deposit into the vault
    MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000_000_000)
        .send()
        .unwrap();

    // Make: maker deposits Mint A into the vault and opens the escrow
    let make_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Make {
            maker,
            mint_a,
            mint_b,
            maker_ata_a,
            escrow,
            vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Make {
            deposit: 10_000_000,   // amount of Mint A the maker locks in the vault
            seed: 124u64,          // unique seed so the same maker can open multiple escrows
            receive: 10_000_000,   // amount of Mint B the maker expects back
            expiration: 17780206209, // deadline after which the escrow can't be taken
        }
        .data(),
    };

    let message = Message::new(&[make_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();
    let transaction = Transaction::new(&[&payer], message, recent_blockhash);
    program.send_transaction(transaction).unwrap();

    // These two ATAs are init_if_needed in the program, so we just derive the
    // addresses here — the program will create them during the Take instruction
    let taker_ata_a = associated_token::get_associated_token_address(&taker, &mint_a);
    let maker_ata_b = associated_token::get_associated_token_address(&maker, &mint_b);

    // Take: taker sends Mint B to maker, then receives Mint A from the vault 
    let take_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Take {
            taker,
            maker,
            mint_a,
            mint_b,
            taker_ata_a,   // where taker will receive Mint A (created if needed)
            taker_ata_b,   // where taker's Mint B payment comes from
            maker_ata_b,   // where maker receives Mint B (created if needed)
            escrow,
            vault,
            token_program: TOKEN_PROGRAM_ID,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Take {}.data(),
    };

    // Note: taker is the fee payer + signer here, not payer, since taker is
    // the one funding the init_if_needed accounts and signing the transfer
    let message = Message::new(&[take_ix], Some(&taker));
    let recent_blockhash = program.latest_blockhash();
    let transaction = Transaction::new(&[&taker_kp], message, recent_blockhash);
    let tx = program.send_transaction(transaction).unwrap();

    msg!("Take transaction successful, CUs: {}", tx.compute_units_consumed);

    // Escrow and vault should be closed out after a successful Take
    assert!(program.get_account(&escrow).is_none());
    assert!(program.get_account(&vault).is_none());

    // Taker should now hold the Mint A that was locked in the vault
    let taker_ata_a_account = program.get_account(&taker_ata_a).unwrap();
    let taker_ata_a_data = spl_token::state::Account::unpack(&taker_ata_a_account.data).unwrap();
    assert_eq!(taker_ata_a_data.amount, 10_000_000);

    // Maker should now hold the Mint B that the taker paid
    let maker_ata_b_account = program.get_account(&maker_ata_b).unwrap();
    let maker_ata_b_data = spl_token::state::Account::unpack(&maker_ata_b_account.data).unwrap();
    assert_eq!(maker_ata_b_data.amount, 10_000_000);
}



#[test]
fn test_take_fails_after_expiration() {
    let (mut program, payer) = setup();
    let maker = payer.pubkey();
    let taker_kp = Keypair::new();
    let taker = taker_kp.pubkey();
    program.airdrop(&taker, 1_000_000_000).unwrap();

    let mint_a = CreateMint::new(&mut program, &payer).decimals(6).authority(&maker).send().unwrap();
    let mint_b = CreateMint::new(&mut program, &payer).decimals(6).authority(&maker).send().unwrap();
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a).owner(&maker).send().unwrap();
    let taker_ata_b = CreateAssociatedTokenAccount::new(&mut program, &taker_kp, &mint_b).owner(&taker).send().unwrap();
    MintTo::new(&mut program, &payer, &mint_b, &taker_ata_b, 10_000_000).send().unwrap();

    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker.as_ref(), &125u64.to_le_bytes()],
        &escrowq32026::id(),
    ).0;
    let vault = associated_token::get_associated_token_address(&escrow, &mint_a);
    MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000_000_000).send().unwrap();

    // Read the current clock so we can base the expiration off "now" rather
    // than a hardcoded future timestamp
    let clock: Clock = program.get_sysvar();

    // Make: set expiration to just 60 seconds from now
    let make_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Make {
            maker, mint_a, mint_b, maker_ata_a, escrow, vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }.to_account_metas(None),
        data: escrowq32026::instruction::Make {
            deposit: 10_000_000,
            seed: 125u64,
            receive: 10_000_000,
            expiration: clock.unix_timestamp + 60, // expires 1 minute from "now"
        }.data(),
    };
    let message = Message::new(&[make_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();
    let transaction = Transaction::new(&[&payer], message, recent_blockhash);
    program.send_transaction(transaction).unwrap();

    // Time travel: push the clock forward past the expiration
    let mut warped_clock: Clock = program.get_sysvar();
    warped_clock.unix_timestamp += 120; // jump forward 2 minutes, past the 60s expiration
    program.set_sysvar::<Clock>(&warped_clock);

    // Take: should now fail because escrow.expiration <= current_time
    let taker_ata_a = associated_token::get_associated_token_address(&taker, &mint_a);
    let maker_ata_b = associated_token::get_associated_token_address(&maker, &mint_b);

    let take_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Take {
            taker, maker, mint_a, mint_b, taker_ata_a, taker_ata_b, maker_ata_b, escrow, vault,
            token_program: TOKEN_PROGRAM_ID,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }.to_account_metas(None),
        data: escrowq32026::instruction::Take {}.data(),
    };
    let message = Message::new(&[take_ix], Some(&taker));
    let recent_blockhash = program.latest_blockhash();
    let transaction = Transaction::new(&[&taker_kp], message, recent_blockhash);

    // Expect this to fail with your EscrowExpired error
    let result = program.send_transaction(transaction);
    assert!(result.is_err(), "Take should fail once the escrow has expired");
}