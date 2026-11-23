<?php
// Roundcube settings for the local GreenMail test server (local development only).
// GreenMail has authentication disabled: sign in with any address and any password.

// Do not send SMTP AUTH; GreenMail accepts mail from anyone.
$config['smtp_user'] = '';
$config['smtp_pass'] = '';

// Public development key: this webmail never runs outside docker compose.
$config['des_key'] = 'cooper-local-dev-only-24c';

$config['product_name'] = 'Cooper local mail';
$config['support_url'] = '';
